use niuma_api::{local_api_addr, spawn_local_api};
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use niuma_core::store::NiumaStore;

pub(crate) fn serve() {
    let store = NiumaStore::new(NiumaStore::default_path());
    match spawn_local_api(store) {
        Ok(handle) => {
            eprintln!(
                "NiumaNotifier Local API listening on http://{}",
                local_api_addr()
            );
            let _ = handle.join();
        }
        Err(error) => {
            let response = ApiResponse::fail(
                ApiErrorCode::ServiceUnavailable,
                format!("启动 Local API 失败：{error}"),
            );
            println!(
                "{}",
                serde_json::to_string_pretty(&response).expect("API envelope 必须可序列化")
            );
        }
    }
}
