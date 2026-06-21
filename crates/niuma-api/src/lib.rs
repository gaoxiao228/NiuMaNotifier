use std::net::TcpListener;
use std::thread;

use niuma_core::config;
use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::store::NiumaStore;

mod approval_proxy_watchdog;
mod handlers;
mod manual_test;
mod response;
mod routes;
mod sse;
mod state;

pub use routes::{app, app_with_bus, app_with_bus_and_plugin_dir};

pub fn local_api_addr() -> String {
    config::local_api_addr()
}

pub fn spawn_local_api(store: NiumaStore) -> std::io::Result<thread::JoinHandle<()>> {
    spawn_local_api_with_bus(store, RuntimeEventBus::new())
}

pub fn spawn_local_api_with_bus(
    store: NiumaStore,
    runtime_events: RuntimeEventBus,
) -> std::io::Result<thread::JoinHandle<()>> {
    let listener = TcpListener::bind(local_api_addr())?;
    listener.set_nonblocking(true)?;
    let handle = thread::spawn(move || {
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("NiumaNotifier Local API runtime error: {error}");
                return;
            }
        };
        runtime.block_on(async move {
            let listener = match tokio::net::TcpListener::from_std(listener) {
                Ok(listener) => listener,
                Err(error) => {
                    eprintln!("NiumaNotifier Local API listener error: {error}");
                    return;
                }
            };
            let mutation_service = niuma_core::state_mutation::StateMutationService::new(
                store.clone(),
                runtime_events.clone(),
            );
            approval_proxy_watchdog::spawn_approval_proxy_watchdog(store.clone(), mutation_service);
            if let Err(error) = axum::serve(listener, app_with_bus(store, runtime_events)).await {
                eprintln!("NiumaNotifier Local API serve error: {error}");
            }
        });
    });
    Ok(handle)
}

#[cfg(test)]
mod tests;
