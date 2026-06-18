use std::net::TcpListener;
use std::thread;

use niuma_core::config;
use niuma_core::runtime_event::RuntimeEventBus;
use niuma_core::store::SqliteStateStore;

mod handlers;
mod manual_test;
mod response;
mod routes;
mod sse;
mod state;

pub use routes::{app, app_with_bus};

pub fn local_api_addr() -> String {
    config::local_api_addr()
}

pub fn spawn_local_api(store: SqliteStateStore) -> std::io::Result<thread::JoinHandle<()>> {
    spawn_local_api_with_bus(store, RuntimeEventBus::new())
}

pub fn spawn_local_api_with_bus(
    store: SqliteStateStore,
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
            if let Err(error) = axum::serve(listener, app_with_bus(store, runtime_events)).await {
                eprintln!("NiumaNotifier Local API serve error: {error}");
            }
        });
    });
    Ok(handle)
}

#[cfg(test)]
mod tests;
