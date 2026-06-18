use std::io::{Read, Write};
use std::net::TcpStream;

use crate::models::NiumaEvent;

pub fn submit_event_to_local_api(addr: &str, event: &NiumaEvent) -> Result<String, String> {
    let body = serde_json::to_string(event).map_err(|error| format!("序列化事件失败：{error}"))?;
    request_local_api(addr, "POST", "/api/v1/events", Some(&body))
}

pub fn get_local_api(addr: &str, path: &str) -> Result<String, String> {
    request_local_api(addr, "GET", path, None)
}

pub fn post_local_api(addr: &str, path: &str, body: Option<&str>) -> Result<String, String> {
    request_local_api(addr, "POST", path, body)
}

fn request_local_api(
    addr: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<String, String> {
    let mut stream =
        TcpStream::connect(addr).map_err(|error| format!("连接 Local API 失败：{error}"))?;

    // CLI/hook 进程保持零依赖 HTTP 客户端，避免为本机请求引入额外 runtime。
    let request = build_request(addr, method, path, body.unwrap_or(""));
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("提交 Local API 请求失败：{error}"))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| format!("读取 Local API 响应失败：{error}"))?;
    response_body(&response)
}

fn build_request(addr: &str, method: &str, path: &str, body: &str) -> String {
    format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn response_body(response: &str) -> Result<String, String> {
    response
        .split("\r\n\r\n")
        .nth(1)
        .map(ToString::to_string)
        .ok_or_else(|| "Local API 响应格式错误".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_post_request_keeps_hook_http_wire_format() {
        let request = build_request(
            "127.0.0.1:27874",
            "POST",
            "/api/v1/events",
            r#"{"id":"event-1"}"#,
        );

        assert_eq!(
            request,
            "POST /api/v1/events HTTP/1.1\r\nHost: 127.0.0.1:27874\r\nContent-Type: application/json\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{\"id\":\"event-1\"}"
        );
    }

    #[test]
    fn get_request_uses_empty_json_body_headers() {
        let request = build_request("127.0.0.1:27874", "GET", "/api/v1/main-state", "");

        assert_eq!(
            request,
            "GET /api/v1/main-state HTTP/1.1\r\nHost: 127.0.0.1:27874\r\nContent-Type: application/json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
    }

    #[test]
    fn response_body_returns_payload_after_header_separator() {
        let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"code\":0}";

        assert_eq!(response_body(response).unwrap(), "{\"code\":0}");
    }

    #[test]
    fn response_body_rejects_malformed_http_response() {
        assert_eq!(
            response_body("HTTP/1.1 200 OK").unwrap_err(),
            "Local API 响应格式错误"
        );
    }
}
