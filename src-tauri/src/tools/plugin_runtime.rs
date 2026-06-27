use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use niuma_api::tool_sessions::{ToolSessionDetailProvider, ToolSessionRegistry};
use niuma_core::plugin::{
    current_plugin_registry, default_non_tool_plugin_enabled, plugin_uses_listener_config,
    resolve_plugin_config, PluginCapability, PluginManifest, PluginRegistry, PluginRuntimeState,
    BUILTIN_BARK_PLUGIN_ID, BUILTIN_NTFY_PLUGIN_ID,
};
use niuma_core::runtime_event::{
    RuntimeEvent, RuntimeEventBus, StateChangeReason, ToolSessionControlChangeReason,
};
use niuma_core::store::NiumaStore;
use niuma_core::tool_session::ToolSessionDetail;
use niuma_core::tool_session_rpc::{
    ProviderRpcNotification, ProviderRpcRequest, ProviderRpcResponse, SessionDetailParams,
    SessionDetailResult, SessionSnapshotParams, SessionSnapshotResult,
};

const FALLBACK_RECONCILE_INTERVAL: Duration = Duration::from_secs(30);
// 首次 snapshot 会扫描本机历史 session；历史较多时 5 秒会误判 provider 启动失败。
const SESSION_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(20);
const SESSION_DETAIL_TIMEOUT: Duration = Duration::from_secs(10);
const PARENT_PID_ENV: &str = "NIUMA_PARENT_PID";
static NEXT_SESSION_PROVIDER_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

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
    session_provider: Option<SessionProviderRuntimeInstance>,
}

struct SpawnedPluginProcess {
    child: Child,
    session_provider: Option<SessionProviderRuntimeInstance>,
}

struct SessionProviderRuntimeInstance {
    tool: niuma_core::models::ToolKind,
    guard: SessionProviderInstanceGuard,
    bootstrap_result: Option<mpsc::Receiver<SessionProviderBootstrapResult>>,
}

#[derive(Clone, Default)]
struct SessionProviderOwnership {
    inner: Arc<Mutex<HashMap<niuma_core::models::ToolKind, u64>>>,
}

enum SessionProviderBootstrapResult {
    Ready,
    Failed(String),
}

#[derive(Clone)]
struct SessionProviderInstanceGuard {
    plugin_id: String,
    instance_id: u64,
    tool: niuma_core::models::ToolKind,
    ownership: SessionProviderOwnership,
    active: Arc<AtomicBool>,
}

impl SessionProviderInstanceGuard {
    fn new(
        plugin_id: String,
        tool: niuma_core::models::ToolKind,
        ownership: SessionProviderOwnership,
    ) -> Self {
        let guard = Self {
            plugin_id,
            instance_id: NEXT_SESSION_PROVIDER_INSTANCE_ID.fetch_add(1, Ordering::Relaxed),
            tool,
            ownership,
            active: Arc::new(AtomicBool::new(true)),
        };
        guard.ownership.register(&guard);
        guard
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    fn invalidate(&self) {
        self.active.store(false, Ordering::Release);
    }
}

impl SessionProviderOwnership {
    fn register(&self, guard: &SessionProviderInstanceGuard) {
        // owner 表只在同一个 manager 内部共享，用于阻止旧实例清理或覆盖新实例。
        self.inner
            .lock()
            .expect("session provider owner lock poisoned")
            .insert(guard.tool.clone(), guard.instance_id);
    }

    fn owns(&self, guard: &SessionProviderInstanceGuard) -> bool {
        guard.is_active()
            && self
                .inner
                .lock()
                .expect("session provider owner lock poisoned")
                .get(&guard.tool)
                .is_some_and(|owner| *owner == guard.instance_id)
    }

    fn release_if_current(&self, guard: &SessionProviderInstanceGuard) -> bool {
        let mut owners = self
            .inner
            .lock()
            .expect("session provider owner lock poisoned");
        if owners
            .get(&guard.tool)
            .is_some_and(|owner| *owner == guard.instance_id)
        {
            owners.remove(&guard.tool);
            return true;
        }
        false
    }

    fn has_owner(&self, tool: &niuma_core::models::ToolKind) -> bool {
        self.inner
            .lock()
            .expect("session provider owner lock poisoned")
            .contains_key(tool)
    }
}

fn run_plugin_manager(
    store: NiumaStore,
    runtime_events: RuntimeEventBus,
    tool_sessions: ToolSessionRegistry,
) {
    let mut managed = HashMap::<String, ManagedPlugin>::new();
    let session_provider_ownership = SessionProviderOwnership::default();
    let mut receiver = runtime_events.subscribe();
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("NiumaNotifier plugin manager runtime not started: {error}");
            return;
        }
    };

    loop {
        reconcile_managed_plugins(
            &store,
            &runtime_events,
            &tool_sessions,
            &mut managed,
            &session_provider_ownership,
        );
        wait_for_plugin_reconcile_signal(&runtime, &mut receiver);
    }
}

fn reconcile_managed_plugins(
    store: &NiumaStore,
    runtime_events: &RuntimeEventBus,
    tool_sessions: &ToolSessionRegistry,
    managed: &mut HashMap<String, ManagedPlugin>,
    session_provider_ownership: &SessionProviderOwnership,
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
            stop_child(store, tool_sessions, &mut entry);
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
                session_provider: None,
            });
        reconcile_managed_plugin_manifest(store, tool_sessions, entry, manifest);
        tick_managed_plugin(
            store,
            runtime_events,
            tool_sessions,
            entry,
            session_provider_ownership,
        );
    }
}

fn reconcile_managed_plugin_manifest(
    store: &NiumaStore,
    tool_sessions: &ToolSessionRegistry,
    entry: &mut ManagedPlugin,
    manifest: PluginManifest,
) {
    if entry.manifest != manifest {
        stop_child(store, tool_sessions, entry);
        entry.manifest = manifest;
        entry.next_start = Instant::now();
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
    runtime_events: &RuntimeEventBus,
    tool_sessions: &ToolSessionRegistry,
    entry: &mut ManagedPlugin,
    session_provider_ownership: &SessionProviderOwnership,
) {
    if !plugin_runtime_enabled(store, &entry.manifest) {
        stop_child(store, tool_sessions, entry);
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

    if handle_session_provider_bootstrap_result(store, tool_sessions, entry) {
        return;
    }

    if let Some(process) = entry.child.as_mut() {
        match process.try_wait() {
            Ok(Some(status)) => {
                let message = format!("插件进程退出：{status}");
                eprintln!("NiumaNotifier plugin {} {message}", entry.manifest.id);
                entry.child = None;
                clear_session_provider_runtime(tool_sessions, &mut entry.session_provider);
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
                clear_session_provider_runtime(tool_sessions, &mut entry.session_provider);
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
        match spawn_plugin_process(
            &entry.manifest,
            runtime_events,
            tool_sessions,
            session_provider_ownership,
        ) {
            Ok(process) => {
                eprintln!("NiumaNotifier plugin {} started", entry.manifest.id);
                entry.child = Some(process.child);
                entry.session_provider = process.session_provider;
                if !is_session_provider {
                    save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::running());
                }
            }
            Err(error) => {
                eprintln!(
                    "NiumaNotifier plugin {} not started: {error}",
                    entry.manifest.id
                );
                clear_session_provider_runtime(tool_sessions, &mut entry.session_provider);
                entry.next_start = Instant::now() + Duration::from_secs(10);
                save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::failed(error));
            }
        }
    }
}

fn handle_session_provider_bootstrap_result(
    store: &NiumaStore,
    tool_sessions: &ToolSessionRegistry,
    entry: &mut ManagedPlugin,
) -> bool {
    let result = match entry
        .session_provider
        .as_ref()
        .and_then(|runtime| runtime.bootstrap_result.as_ref())
        .map(|receiver| receiver.try_recv())
    {
        Some(Ok(result)) => result,
        Some(Err(mpsc::TryRecvError::Empty)) | None => return false,
        Some(Err(mpsc::TryRecvError::Disconnected)) => SessionProviderBootstrapResult::Failed(
            "session provider snapshot bootstrap 结果通道已关闭".to_string(),
        ),
    };

    match result {
        SessionProviderBootstrapResult::Ready => {
            if let Some(runtime) = entry.session_provider.as_mut() {
                runtime.bootstrap_result = None;
            }
            save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::running());
            false
        }
        SessionProviderBootstrapResult::Failed(error) => {
            stop_session_provider_after_bootstrap_failure(store, tool_sessions, entry, error);
            true
        }
    }
}

fn stop_session_provider_after_bootstrap_failure(
    store: &NiumaStore,
    tool_sessions: &ToolSessionRegistry,
    entry: &mut ManagedPlugin,
    error: String,
) {
    eprintln!(
        "NiumaNotifier plugin {} bootstrap failed: {error}",
        entry.manifest.id
    );
    if let Some(mut process) = entry.child.take() {
        if let Err(kill_error) = process.kill() {
            eprintln!(
                "NiumaNotifier plugin {} stop after bootstrap failed: {kill_error}",
                entry.manifest.id
            );
        }
        let _ = process.wait();
    }
    clear_session_provider_runtime(tool_sessions, &mut entry.session_provider);
    entry.next_start = Instant::now() + Duration::from_secs(5);
    save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::failed(error));
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
    manifest: &PluginManifest,
    runtime_events: &RuntimeEventBus,
    tool_sessions: &ToolSessionRegistry,
    session_provider_ownership: &SessionProviderOwnership,
) -> Result<SpawnedPluginProcess, String> {
    let mut child = build_plugin_command_for_runtime(manifest)?
        .spawn()
        .map_err(|error| format!("启动插件进程失败：{error}"))?;
    let mut session_provider = None;
    if is_session_provider_manifest(manifest) {
        match bootstrap_session_provider(
            manifest,
            &mut child,
            runtime_events,
            tool_sessions,
            session_provider_ownership,
        ) {
            Ok(runtime) => session_provider = Some(runtime),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
    }
    Ok(SpawnedPluginProcess {
        child,
        session_provider,
    })
}

#[cfg(test)]
fn build_plugin_command(manifest: &PluginManifest) -> Result<Command, String> {
    build_plugin_command_with_stdio(manifest, PluginStdioMode::Null)
}

fn build_plugin_command_for_runtime(manifest: &PluginManifest) -> Result<Command, String> {
    build_plugin_command_with_stdio(manifest, plugin_stdio_mode(manifest))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PluginStdioMode {
    Null,
    ProviderJsonLines,
}

fn plugin_stdio_mode(manifest: &PluginManifest) -> PluginStdioMode {
    if is_session_provider_manifest(manifest) {
        PluginStdioMode::ProviderJsonLines
    } else {
        PluginStdioMode::Null
    }
}

fn build_plugin_command_with_stdio(
    manifest: &PluginManifest,
    stdio_mode: PluginStdioMode,
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
        .stdin(if stdio_mode == PluginStdioMode::ProviderJsonLines {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(if stdio_mode == PluginStdioMode::ProviderJsonLines {
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
    manifest: &PluginManifest,
    child: &mut Child,
    runtime_events: &RuntimeEventBus,
    tool_sessions: &ToolSessionRegistry,
    session_provider_ownership: &SessionProviderOwnership,
) -> Result<SessionProviderRuntimeInstance, String> {
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
    let guard = SessionProviderInstanceGuard::new(
        manifest.id.clone(),
        tool.clone(),
        session_provider_ownership.clone(),
    );
    let pending = ProviderPendingResponses::default();
    let client = Arc::new(ProviderProcessClient::new(
        manifest.id.clone(),
        stdin,
        pending.clone(),
    ));
    spawn_provider_stdout_reader(
        manifest.id.clone(),
        stdout,
        guard.clone(),
        runtime_events.clone(),
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
    let (bootstrap_sender, bootstrap_receiver) = mpsc::channel();
    if let Err(error) = spawn_session_snapshot_bootstrap_thread(
        manifest.id.clone(),
        guard.clone(),
        runtime_events.clone(),
        tool_sessions.clone(),
        bootstrap_sender,
        move || snapshot_client.session_snapshot(snapshot_tool),
    ) {
        let owns_tool = guard.ownership.release_if_current(&guard);
        guard.invalidate();
        if owns_tool {
            tool_sessions.unregister_detail_provider(&tool);
            tool_sessions.clear_snapshot(&tool);
        }
        return Err(error);
    }
    Ok(SessionProviderRuntimeInstance {
        tool,
        guard,
        bootstrap_result: Some(bootstrap_receiver),
    })
}

fn spawn_session_snapshot_bootstrap_thread<F>(
    plugin_id: String,
    guard: SessionProviderInstanceGuard,
    runtime_events: RuntimeEventBus,
    tool_sessions: ToolSessionRegistry,
    result_sender: mpsc::Sender<SessionProviderBootstrapResult>,
    fetch_snapshot: F,
) -> Result<thread::JoinHandle<()>, String>
where
    F: FnOnce() -> Result<SessionSnapshotResult, String> + Send + 'static,
{
    thread::Builder::new()
        .name(format!("plugin-provider-bootstrap-{plugin_id}"))
        .spawn(move || match fetch_snapshot() {
            Ok(snapshot) if snapshot.tool == guard.tool => {
                if replace_session_provider_snapshot(&tool_sessions, &guard, snapshot) {
                    send_session_provider_bootstrap_result(
                        &runtime_events,
                        &result_sender,
                        SessionProviderBootstrapResult::Ready,
                    );
                }
            }
            Ok(snapshot) => {
                let message = format!(
                    "provider snapshot tool 不匹配：expected={} actual={}",
                    guard.tool.as_str(),
                    snapshot.tool.as_str()
                );
                eprintln!("NiumaNotifier provider {plugin_id} {message}");
                if guard.is_active() {
                    fail_session_provider_bootstrap(
                        &runtime_events,
                        &result_sender,
                        &guard,
                        message,
                    );
                }
            }
            Err(error) => {
                if guard.is_active() {
                    fail_session_provider_bootstrap(&runtime_events, &result_sender, &guard, error);
                }
            }
        })
        .map_err(|error| format!("session provider snapshot bootstrap 未启动：{error}"))
}

fn send_session_provider_bootstrap_result(
    runtime_events: &RuntimeEventBus,
    result_sender: &mpsc::Sender<SessionProviderBootstrapResult>,
    result: SessionProviderBootstrapResult,
) {
    if result_sender.send(result).is_ok() {
        // 复用现有配置变更事件作为 manager 唤醒信号，避免新增 enum 扩大影响面。
        runtime_events.publish_state_changed(StateChangeReason::PluginConfigChanged);
    }
}

fn fail_session_provider_bootstrap(
    runtime_events: &RuntimeEventBus,
    result_sender: &mpsc::Sender<SessionProviderBootstrapResult>,
    guard: &SessionProviderInstanceGuard,
    error: String,
) {
    // Failed 入队前先撤销当前实例写入资格，堵住 manager 清理前的 stdout 通知窗口。
    let _ = guard.ownership.release_if_current(guard);
    guard.invalidate();
    send_session_provider_bootstrap_result(
        runtime_events,
        result_sender,
        SessionProviderBootstrapResult::Failed(error),
    );
}

fn spawn_provider_stdout_reader(
    plugin_id: String,
    stdout: ChildStdout,
    guard: SessionProviderInstanceGuard,
    runtime_events: RuntimeEventBus,
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
                    Ok(line) => handle_provider_stdout_line(
                        &tool_sessions,
                        Some(&runtime_events),
                        Some(&pending),
                        Some(&guard),
                        &line,
                    ),
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
    runtime_events: Option<&RuntimeEventBus>,
    pending: Option<&ProviderPendingResponses>,
    provider_guard: Option<&SessionProviderInstanceGuard>,
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
            Ok(notification) => handle_provider_notification(
                tool_sessions,
                runtime_events,
                provider_guard,
                notification,
            ),
            Err(error) => eprintln!("NiumaNotifier provider notification parse failed: {error}"),
        }
        return;
    }

    eprintln!("NiumaNotifier provider stdout line is neither response nor notification: {line}");
}

fn handle_provider_notification(
    tool_sessions: &ToolSessionRegistry,
    runtime_events: Option<&RuntimeEventBus>,
    provider_guard: Option<&SessionProviderInstanceGuard>,
    notification: ProviderRpcNotification,
) {
    match notification.method.as_str() {
        "session_snapshot_updated" => match notification.params_as::<SessionSnapshotResult>() {
            Ok(snapshot) => {
                let Some(provider_guard) = provider_guard else {
                    eprintln!(
                        "NiumaNotifier provider snapshot notification ignored: missing guard"
                    );
                    return;
                };
                if replace_session_provider_snapshot(tool_sessions, provider_guard, snapshot) {
                    if let Some(runtime_events) = runtime_events {
                        runtime_events.publish_tool_session_control_changed(
                            provider_guard.tool.clone(),
                            None,
                            None,
                            None,
                            ToolSessionControlChangeReason::SnapshotRefreshed,
                        );
                    }
                } else {
                    eprintln!(
                        "NiumaNotifier provider {} snapshot notification ignored",
                        provider_guard.plugin_id
                    );
                }
            }
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
        limit: usize,
        cursor: Option<String>,
    ) -> Result<ToolSessionDetail, String> {
        let result = self.client.session_detail(SessionDetailParams {
            tool: tool.clone(),
            session_id: session_id.to_string(),
            // API 层已经完成缺省值和上限归一化，RPC 只把确定的分页数量传给 provider。
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

fn replace_session_provider_snapshot(
    tool_sessions: &ToolSessionRegistry,
    guard: &SessionProviderInstanceGuard,
    snapshot: SessionSnapshotResult,
) -> bool {
    if !guard.is_active() {
        eprintln!(
            "NiumaNotifier provider {} instance {} snapshot ignored: inactive",
            guard.plugin_id, guard.instance_id
        );
        return false;
    }
    if !guard.ownership.owns(guard) {
        eprintln!(
            "NiumaNotifier provider {} instance {} snapshot ignored: not owner",
            guard.plugin_id, guard.instance_id
        );
        return false;
    }
    if snapshot.tool != guard.tool {
        eprintln!(
            "NiumaNotifier provider {} snapshot tool mismatch: expected={} actual={}",
            guard.plugin_id,
            guard.tool.as_str(),
            snapshot.tool.as_str()
        );
        return false;
    }
    // snapshot 只能由当前 provider 实例写入，避免旧 stdout 线程覆盖新实例缓存。
    tool_sessions.replace_snapshot(guard.tool.clone(), snapshot.sessions);
    true
}

fn clear_session_provider_runtime(
    tool_sessions: &ToolSessionRegistry,
    runtime: &mut Option<SessionProviderRuntimeInstance>,
) {
    let Some(runtime) = runtime.take() else {
        return;
    };
    let owns_tool = runtime.guard.ownership.release_if_current(&runtime.guard);
    runtime.guard.invalidate();
    if owns_tool || !runtime.guard.ownership.has_owner(&runtime.tool) {
        // 当前实例提前释放 owner 时仍要清 registry；若已有新 owner，则不能误清新 provider。
        tool_sessions.unregister_detail_provider(&runtime.tool);
        tool_sessions.clear_snapshot(&runtime.tool);
    }
}

fn stop_child(store: &NiumaStore, tool_sessions: &ToolSessionRegistry, entry: &mut ManagedPlugin) {
    clear_session_provider_runtime(tool_sessions, &mut entry.session_provider);
    let Some(mut process) = entry.child.take() else {
        return;
    };
    save_runtime_state(store, &entry.manifest.id, PluginRuntimeState::stopping());
    if let Err(error) = process.kill() {
        eprintln!(
            "NiumaNotifier plugin {} stop failed: {error}",
            entry.manifest.id
        );
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
        let codex = manifests
            .iter()
            .find(|manifest| manifest.id == "builtin-codex")
            .unwrap();
        assert_eq!(codex.source, PluginSource::Builtin);
        assert!(codex.capabilities.contains(&PluginCapability::EventWatcher));
        assert!(codex
            .capabilities
            .contains(&PluginCapability::ToolSessionListProvider));
        let claude_code = manifests
            .iter()
            .find(|manifest| manifest.id == "builtin-claude-code")
            .unwrap();
        assert_eq!(claude_code.source, PluginSource::Builtin);
        assert!(claude_code
            .capabilities
            .contains(&PluginCapability::EventWatcher));
        assert!(claude_code
            .capabilities
            .contains(&PluginCapability::ToolSessionListProvider));
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
        let runtime_events = RuntimeEventBus::new();
        let mut receiver = runtime_events.subscribe();
        let line = serde_json::json!({
            "method": "session_snapshot_updated",
            "params": {
                "tool": "codex",
                "sessions": [provider_test_session("s1")]
            }
        })
        .to_string();
        let guard = active_provider_guard("provider-notify", ToolKind::Codex);

        handle_provider_stdout_line(&registry, Some(&runtime_events), None, Some(&guard), &line);

        let sessions = registry
            .list(niuma_api::tool_sessions::ToolSessionListQuery {
                tool: Some("codex".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "s1");
        assert!(matches!(
            receiver.try_recv().unwrap(),
            RuntimeEvent::ToolSessionControlChanged {
                tool: ToolKind::Codex,
                reason: ToolSessionControlChangeReason::SnapshotRefreshed,
                ..
            }
        ));
    }

    #[test]
    fn invalid_provider_stdout_line_does_not_block_later_notification() {
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        let guard = active_provider_guard("provider-notify", ToolKind::Codex);

        // provider stdout 可能混入日志或半行 JSON；解析失败不能终止后续合法通知处理。
        handle_provider_stdout_line(&registry, None, None, Some(&guard), "{not-json");
        handle_provider_stdout_line(
            &registry,
            None,
            None,
            Some(&guard),
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
    fn inactive_provider_notification_does_not_write_snapshot() {
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        let guard = active_provider_guard("provider-old", ToolKind::Codex);
        guard.invalidate();

        // 旧 stdout reader 可能晚于新实例退出，inactive guard 必须阻止旧 snapshot 写入。
        handle_provider_stdout_line(
            &registry,
            None,
            None,
            Some(&guard),
            &serde_json::json!({
                "method": "session_snapshot_updated",
                "params": {
                    "tool": "codex",
                    "sessions": [provider_test_session("old-session")]
                }
            })
            .to_string(),
        );

        assert!(registry
            .find_session(&ToolKind::Codex, "old-session")
            .is_none());
    }

    #[test]
    fn provider_notification_with_unexpected_tool_does_not_write_snapshot() {
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        let guard = active_provider_guard("provider-tool-mismatch", ToolKind::Codex);
        let unexpected_tool = ToolKind::Custom("cursor".to_string());

        // provider manifest 只允许上报 expected tool，错 tool 通知不能替换其他工具缓存。
        handle_provider_stdout_line(
            &registry,
            None,
            None,
            Some(&guard),
            &serde_json::json!({
                "method": "session_snapshot_updated",
                "params": {
                    "tool": unexpected_tool,
                    "sessions": [provider_test_session("wrong-tool-session")]
                }
            })
            .to_string(),
        );

        assert!(registry
            .find_session(&ToolKind::Codex, "wrong-tool-session")
            .is_none());
        assert!(registry
            .find_session(
                &ToolKind::Custom("cursor".to_string()),
                "wrong-tool-session"
            )
            .is_none());
    }

    #[test]
    fn invalid_provider_stdout_line_does_not_block_later_response() {
        let pending = ProviderPendingResponses::default();
        let receiver = pending.insert("req-after-invalid".to_string());

        // provider stdout 混入非法 JSON 后，合法 response 仍必须完成对应 pending 请求。
        handle_provider_stdout_line(
            &niuma_api::tool_sessions::ToolSessionRegistry::new(),
            None,
            Some(&pending),
            None,
            "not-json",
        );
        handle_provider_stdout_line(
            &niuma_api::tool_sessions::ToolSessionRegistry::new(),
            None,
            Some(&pending),
            None,
            &serde_json::json!({
                "id": "req-after-invalid",
                "result": {
                    "tool": "codex",
                    "sessions": []
                }
            })
            .to_string(),
        );

        let response = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(response.id, "req-after-invalid");
        assert_eq!(pending.len(), 0);
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
            None,
            Some(&pending),
            None,
            &line,
        );

        let response = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(response.id, "req-1");
        assert!(response.error.is_none());
    }

    #[test]
    fn pending_response_timeout_removes_pending_entry() {
        let pending = ProviderPendingResponses::default();
        let mut child = Command::new("/bin/sleep")
            .arg("1")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let client =
            ProviderProcessClient::new("timeout-provider".to_string(), stdin, pending.clone());

        // 走 ProviderProcessClient::call 的 recv_timeout 分支，验证真实超时清理 pending。
        let error = client
            .call::<_, SessionSnapshotResult>(
                "session_snapshot",
                SessionSnapshotParams {
                    tool: ToolKind::Codex,
                },
                Duration::from_millis(20),
            )
            .unwrap_err();

        assert!(error.contains("provider 请求超时"));
        assert_eq!(pending.len(), 0);
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn session_snapshot_bootstrap_returns_before_snapshot_fetch_finishes() {
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        let guard = active_provider_guard("provider-async", ToolKind::Codex);
        let runtime_events = RuntimeEventBus::new();
        let (sender, receiver) = mpsc::channel();
        let started_at = Instant::now();

        let handle = spawn_session_snapshot_bootstrap_thread(
            "provider-async".to_string(),
            guard,
            runtime_events,
            registry.clone(),
            sender,
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
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
            SessionProviderBootstrapResult::Ready
        ));
        assert_eq!(
            registry
                .find_session(&ToolKind::Codex, "s1")
                .unwrap()
                .session_id,
            "s1"
        );
    }

    #[test]
    fn session_snapshot_bootstrap_result_wakes_plugin_manager() {
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        let guard = active_provider_guard("provider-wakeup", ToolKind::Codex);
        let runtime_events = RuntimeEventBus::new();
        let mut event_receiver = runtime_events.subscribe();
        let (sender, receiver) = mpsc::channel();

        let handle = spawn_session_snapshot_bootstrap_thread(
            "provider-wakeup".to_string(),
            guard,
            runtime_events,
            registry,
            sender,
            || {
                Ok(SessionSnapshotResult {
                    tool: ToolKind::Codex,
                    sessions: Vec::new(),
                })
            },
        )
        .unwrap();
        handle.join().unwrap();

        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
            SessionProviderBootstrapResult::Ready
        ));
        assert!(matches!(
            event_receiver.try_recv().unwrap(),
            RuntimeEvent::StateChanged {
                reason: StateChangeReason::PluginConfigChanged,
                ..
            }
        ));
    }

    #[test]
    fn failed_session_snapshot_bootstrap_result_wakes_plugin_manager() {
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        let guard = active_provider_guard("provider-failed-wakeup", ToolKind::Codex);
        let runtime_events = RuntimeEventBus::new();
        let mut event_receiver = runtime_events.subscribe();
        let (sender, receiver) = mpsc::channel();

        let handle = spawn_session_snapshot_bootstrap_thread(
            "provider-failed-wakeup".to_string(),
            guard,
            runtime_events,
            registry,
            sender,
            || Err("provider 请求超时：session_snapshot".to_string()),
        )
        .unwrap();
        handle.join().unwrap();

        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
            SessionProviderBootstrapResult::Failed(error) if error.contains("provider 请求超时")
        ));
        assert!(matches!(
            event_receiver.try_recv().unwrap(),
            RuntimeEvent::StateChanged {
                reason: StateChangeReason::PluginConfigChanged,
                ..
            }
        ));
    }

    #[test]
    fn failed_session_snapshot_bootstrap_reports_failure_without_clearing_registry() {
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        let guard = active_provider_guard("provider-failed", ToolKind::Codex);
        let runtime_events = RuntimeEventBus::new();
        let (sender, receiver) = mpsc::channel();
        registry.replace_snapshot(
            ToolKind::Codex,
            vec![provider_test_session_item("stale-session")],
        );
        registry.register_detail_provider(ToolKind::Codex, Arc::new(FakeToolSessionDetailProvider));

        let handle = spawn_session_snapshot_bootstrap_thread(
            "provider-failed".to_string(),
            guard,
            runtime_events,
            registry.clone(),
            sender,
            || Err("provider 请求超时：session_snapshot".to_string()),
        )
        .unwrap();
        handle.join().unwrap();

        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
            SessionProviderBootstrapResult::Failed(error) if error.contains("provider 请求超时")
        ));
        assert!(registry
            .find_session(&ToolKind::Codex, "stale-session")
            .is_some());
        assert!(registry
            .detail(&ToolKind::Codex, "stale-session", 100, None)
            .is_ok());
    }

    #[test]
    fn failed_bootstrap_guard_cannot_write_snapshot_before_manager_cleanup() {
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        let guard = active_provider_guard("provider-failed-window", ToolKind::Codex);
        let runtime_events = RuntimeEventBus::new();
        let (sender, receiver) = mpsc::channel();

        let handle = spawn_session_snapshot_bootstrap_thread(
            "provider-failed-window".to_string(),
            guard.clone(),
            runtime_events,
            registry.clone(),
            sender,
            || Err("provider 请求超时：session_snapshot".to_string()),
        )
        .unwrap();
        handle.join().unwrap();
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
            SessionProviderBootstrapResult::Failed(error) if error.contains("provider 请求超时")
        ));

        // manager 尚未清理时，失败实例的 stdout 线程仍可能读到通知，guard 必须先阻止写入。
        handle_provider_stdout_line(
            &registry,
            None,
            None,
            Some(&guard),
            &serde_json::json!({
                "method": "session_snapshot_updated",
                "params": {
                    "tool": "codex",
                    "sessions": [provider_test_session("failed-window-session")]
                }
            })
            .to_string(),
        );

        assert!(registry
            .find_session(&ToolKind::Codex, "failed-window-session")
            .is_none());
    }

    #[test]
    fn inactive_bootstrap_result_does_not_clear_new_provider_snapshot() {
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        let old_guard = active_provider_guard("provider-old", ToolKind::Codex);
        let runtime_events = RuntimeEventBus::new();
        let (sender, receiver) = mpsc::channel();
        old_guard.invalidate();
        registry.replace_snapshot(
            ToolKind::Codex,
            vec![provider_test_session_item("new-session")],
        );
        registry.register_detail_provider(ToolKind::Codex, Arc::new(FakeToolSessionDetailProvider));

        let handle = spawn_session_snapshot_bootstrap_thread(
            "provider-old".to_string(),
            old_guard,
            runtime_events,
            registry.clone(),
            sender,
            || Err("provider 请求超时：session_snapshot".to_string()),
        )
        .unwrap();
        handle.join().unwrap();

        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
        assert!(registry
            .find_session(&ToolKind::Codex, "new-session")
            .is_some());
        assert!(registry
            .detail(&ToolKind::Codex, "new-session", 100, None)
            .is_ok());
    }

    #[test]
    fn old_provider_runtime_cleanup_does_not_clear_new_provider_owner() {
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        let ownership = SessionProviderOwnership::default();
        let old_guard = SessionProviderInstanceGuard::new(
            "provider-old".to_string(),
            ToolKind::Codex,
            ownership.clone(),
        );
        let _new_guard = SessionProviderInstanceGuard::new(
            "provider-new".to_string(),
            ToolKind::Codex,
            ownership,
        );
        let mut old_runtime = Some(SessionProviderRuntimeInstance {
            tool: ToolKind::Codex,
            guard: old_guard,
            bootstrap_result: None,
        });
        registry.replace_snapshot(
            ToolKind::Codex,
            vec![provider_test_session_item("new-session")],
        );
        registry.register_detail_provider(ToolKind::Codex, Arc::new(FakeToolSessionDetailProvider));

        clear_session_provider_runtime(&registry, &mut old_runtime);

        assert!(registry
            .find_session(&ToolKind::Codex, "new-session")
            .is_some());
        assert!(registry
            .detail(&ToolKind::Codex, "new-session", 100, None)
            .is_ok());
    }

    #[test]
    fn bootstrap_failure_stops_child_and_schedules_retry() {
        let store = NiumaStore::new(test_sqlite_path("session_snapshot_manager_failed"));
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        let mut enabled = BTreeMap::new();
        enabled.insert("codex-session-provider".to_string(), true);
        store.save_plugin_enabled_map(&enabled).unwrap();
        registry.replace_snapshot(
            ToolKind::Codex,
            vec![provider_test_session_item("stale-session")],
        );
        registry.register_detail_provider(ToolKind::Codex, Arc::new(FakeToolSessionDetailProvider));
        let child = Command::new("/bin/sleep")
            .arg("5")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let guard = active_provider_guard("codex-session-provider", ToolKind::Codex);
        let (sender, receiver) = mpsc::channel();
        sender
            .send(SessionProviderBootstrapResult::Failed(
                "provider 请求超时：session_snapshot".to_string(),
            ))
            .unwrap();
        let mut entry = ManagedPlugin {
            manifest: session_provider_manifest("codex-session-provider"),
            child: Some(child),
            next_start: Instant::now(),
            session_provider: Some(SessionProviderRuntimeInstance {
                tool: ToolKind::Codex,
                guard,
                bootstrap_result: Some(receiver),
            }),
        };
        let started_before_tick = Instant::now();

        tick_managed_plugin(
            &store,
            &RuntimeEventBus::new(),
            &registry,
            &mut entry,
            &SessionProviderOwnership::default(),
        );

        assert!(entry.child.is_none());
        assert!(entry.session_provider.is_none());
        assert!(entry.next_start > started_before_tick);
        assert!(registry
            .find_session(&ToolKind::Codex, "stale-session")
            .is_none());
        assert_eq!(
            registry
                .detail(&ToolKind::Codex, "stale-session", 100, None)
                .unwrap_err(),
            "session detail provider 尚未就绪"
        );
        let state = store
            .plugin_runtime_states()
            .unwrap()
            .remove("codex-session-provider")
            .unwrap();
        assert_eq!(
            state.status,
            niuma_core::plugin::PluginRuntimeStatus::Failed
        );
    }

    #[test]
    fn disabled_provider_runtime_clears_snapshot_and_detail_provider() {
        let store = NiumaStore::new(test_sqlite_path("provider_disabled_cleanup"));
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        seed_codex_session_provider_registry(&registry, "disabled-session");
        let mut enabled = BTreeMap::new();
        enabled.insert("codex-session-provider".to_string(), false);
        store.save_plugin_enabled_map(&enabled).unwrap();
        let mut entry = managed_provider_entry(
            "codex-session-provider",
            sleep_child(),
            SessionProviderOwnership::default(),
        );

        // 直接进入 disabled 分支，验证 stop_child 会清理 provider runtime 持有的缓存。
        tick_managed_plugin(
            &store,
            &RuntimeEventBus::new(),
            &registry,
            &mut entry,
            &SessionProviderOwnership::default(),
        );

        assert!(entry.child.is_none());
        assert!(entry.session_provider.is_none());
        assert_codex_session_provider_cleared(&registry, "disabled-session");
        let state = store
            .plugin_runtime_states()
            .unwrap()
            .remove("codex-session-provider")
            .unwrap();
        assert_eq!(
            state.status,
            niuma_core::plugin::PluginRuntimeStatus::Stopped
        );
    }

    #[test]
    fn manifest_change_stops_old_provider_and_clears_session_registry() {
        let store = NiumaStore::new(test_sqlite_path("provider_manifest_change_cleanup"));
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        seed_codex_session_provider_registry(&registry, "manifest-session");
        let mut entry = managed_provider_entry(
            "codex-session-provider",
            sleep_child(),
            SessionProviderOwnership::default(),
        );
        let replacement = notification_consumer_manifest("codex-session-provider");

        // 抽出的 manifest reconcile helper 覆盖生产路径里的旧 entry stop 行为。
        reconcile_managed_plugin_manifest(&store, &registry, &mut entry, replacement.clone());

        assert!(entry.child.is_none());
        assert!(entry.session_provider.is_none());
        assert_eq!(entry.manifest, replacement);
        assert_codex_session_provider_cleared(&registry, "manifest-session");
    }

    #[test]
    fn exited_provider_child_clears_snapshot_and_detail_provider() {
        let store = NiumaStore::new(test_sqlite_path("provider_child_exit_cleanup"));
        let registry = niuma_api::tool_sessions::ToolSessionRegistry::new();
        seed_codex_session_provider_registry(&registry, "exited-session");
        let mut enabled = BTreeMap::new();
        enabled.insert("codex-session-provider".to_string(), true);
        store.save_plugin_enabled_map(&enabled).unwrap();
        let mut entry = managed_provider_entry(
            "codex-session-provider",
            true_child(),
            SessionProviderOwnership::default(),
        );
        let started_before_tick = Instant::now();

        // /bin/sh -c true 很快退出；循环 tick 直到覆盖 try_wait Some(status) 分支。
        while entry.child.is_some() && started_before_tick.elapsed() < Duration::from_secs(1) {
            tick_managed_plugin(
                &store,
                &RuntimeEventBus::new(),
                &registry,
                &mut entry,
                &SessionProviderOwnership::default(),
            );
            if entry.child.is_some() {
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        assert!(entry.child.is_none());
        assert!(entry.session_provider.is_none());
        assert!(entry.next_start > started_before_tick);
        assert_codex_session_provider_cleared(&registry, "exited-session");
        let state = store
            .plugin_runtime_states()
            .unwrap()
            .remove("codex-session-provider")
            .unwrap();
        assert_eq!(
            state.status,
            niuma_core::plugin::PluginRuntimeStatus::Failed
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
    fn plain_plugin_command_uses_null_stdio_mode() {
        let manifest = notification_consumer_manifest("plain-notification");

        // 普通插件不能占用 provider JSON Lines stdin/stdout 通道。
        assert_eq!(plugin_stdio_mode(&manifest), PluginStdioMode::Null);
    }

    #[test]
    fn session_provider_command_uses_piped_stdio_mode() {
        let manifest = session_provider_manifest("codex-session-provider");

        // session provider 需要 JSON Lines 通信，必须保留 piped stdin/stdout。
        assert_eq!(
            plugin_stdio_mode(&manifest),
            PluginStdioMode::ProviderJsonLines
        );
    }

    #[test]
    fn merged_builtin_codex_uses_piped_stdio_mode() {
        let manifest = PluginRegistry::with_builtin_plugins()
            .plugin_by_id("builtin-codex")
            .unwrap()
            .clone();

        // 合并后的 Codex 插件同时承载 provider RPC，stdout 不能再作为普通日志输出。
        assert_eq!(
            plugin_stdio_mode(&manifest),
            PluginStdioMode::ProviderJsonLines
        );
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

    fn managed_provider_entry(
        id: &str,
        child: Child,
        ownership: SessionProviderOwnership,
    ) -> ManagedPlugin {
        let guard = SessionProviderInstanceGuard::new(id.to_string(), ToolKind::Codex, ownership);
        ManagedPlugin {
            manifest: session_provider_manifest(id),
            child: Some(child),
            next_start: Instant::now() + Duration::from_secs(60),
            session_provider: Some(SessionProviderRuntimeInstance {
                tool: ToolKind::Codex,
                guard,
                bootstrap_result: None,
            }),
        }
    }

    fn sleep_child() -> Child {
        // 长命进程用于证明 stop_child 会 kill/wait，并把 entry.child 置空。
        Command::new("/bin/sleep")
            .arg("5")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    fn true_child() -> Child {
        // 短命进程用于稳定触发 manager 的 try_wait Some(status) 清理分支。
        Command::new("/bin/sh")
            .arg("-c")
            .arg("true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    fn seed_codex_session_provider_registry(
        registry: &niuma_api::tool_sessions::ToolSessionRegistry,
        session_id: &str,
    ) {
        registry.replace_snapshot(
            ToolKind::Codex,
            vec![provider_test_session_item(session_id)],
        );
        registry.register_detail_provider(ToolKind::Codex, Arc::new(FakeToolSessionDetailProvider));
    }

    fn assert_codex_session_provider_cleared(
        registry: &niuma_api::tool_sessions::ToolSessionRegistry,
        session_id: &str,
    ) {
        // snapshot 和 detail provider 必须同时消失，才算 manager 入口完成 provider 清理。
        assert!(registry
            .find_session(&ToolKind::Codex, session_id)
            .is_none());
        assert_eq!(
            registry
                .detail(&ToolKind::Codex, session_id, 100, None)
                .unwrap_err(),
            "session detail provider 尚未就绪"
        );
    }

    fn active_provider_guard(id: &str, tool: ToolKind) -> SessionProviderInstanceGuard {
        SessionProviderInstanceGuard::new(id.to_string(), tool, SessionProviderOwnership::default())
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
            _limit: usize,
            _cursor: Option<String>,
        ) -> Result<ToolSessionDetail, String> {
            Ok(ToolSessionDetail {
                tool: ToolKind::Codex,
                session_id: session_id.to_string(),
                project_path: "/tmp/demo".to_string(),
                project_name: "demo".to_string(),
                is_subagent: false,
                parent_session_id: None,
                normalized_session_id: Some(session_id.to_string()),
                session_scope: Some(niuma_core::tool_session::ToolSessionScope::Main),
                agent_nickname: None,
                agent_role: None,
                normalization_status: Some(
                    niuma_core::tool_session::ToolSessionNormalizationStatus::Resolved,
                ),
                control: None,
                runtime_status: None,
                runtime_last_event_id: None,
                runtime_last_activity_at: None,
                pending_action: None,
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
