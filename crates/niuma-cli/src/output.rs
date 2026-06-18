use niuma_core::api_response::ApiResponse;

pub(crate) fn print_response(output: &ApiResponse<serde_json::Value>) {
    println!(
        "{}",
        serde_json::to_string_pretty(output).expect("API envelope 必须可序列化")
    );
}
