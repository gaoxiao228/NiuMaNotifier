use serde_json::json;

use crate::api_response::{ApiErrorCode, ApiResponse};

#[test]
fn success_response_has_standard_shape() {
    let response = ApiResponse::ok(json!({ "status": "idle" }));
    let encoded = serde_json::to_value(response).unwrap();

    assert_eq!(encoded["code"], 0);
    assert_eq!(encoded["message"], "ok");
    assert_eq!(encoded["data"], json!({ "status": "idle" }));
}

#[test]
fn business_failure_uses_non_zero_code_and_null_data() {
    let response = ApiResponse::fail(ApiErrorCode::BusinessValidation, "参数错误");
    let encoded = serde_json::to_value(response).unwrap();

    assert_eq!(encoded["code"], 100101);
    assert_eq!(encoded["message"], "参数错误");
    assert!(encoded["data"].is_null());
}

#[test]
fn route_not_found_uses_standard_error_code() {
    assert_eq!(ApiErrorCode::RouteNotFound.code(), 900005);
}

#[test]
fn system_error_uses_standard_error_code() {
    assert_eq!(ApiErrorCode::System.code(), 900001);
}
