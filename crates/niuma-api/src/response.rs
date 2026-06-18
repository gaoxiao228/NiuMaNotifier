use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use niuma_core::api_response::{ApiErrorCode, ApiResponse};
use serde_json::json;

pub(crate) async fn preflight() -> Response {
    json_response(200, ApiResponse::ok(json!({})))
}

pub(crate) async fn route_not_found() -> Response {
    json_response(
        404,
        ApiResponse::fail(ApiErrorCode::RouteNotFound, "接口不存在"),
    )
}

pub(crate) fn json_response<T: serde::Serialize>(
    status: u16,
    response: ApiResponse<T>,
) -> Response {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = serde_json::to_string(&response).expect("API response 必须可序列化");
    let mut response = (status, body).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    apply_cors_headers(headers);
    response
}

pub(crate) fn apply_cors_headers(headers: &mut HeaderMap) {
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Content-Type"),
    );
}
