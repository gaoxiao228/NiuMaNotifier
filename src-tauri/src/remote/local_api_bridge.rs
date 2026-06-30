use std::collections::BTreeMap;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct LocalApiRequestParams {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub body: Option<Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LocalApiResponsePayload {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct LocalApiStreamCloseParams {
    pub stream_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedSseEvent {
    pub id: Option<String>,
    pub event: String,
    pub data: Value,
}

#[derive(Debug, Default)]
pub struct StreamNotificationSequence {
    next: u64,
}

impl StreamNotificationSequence {
    pub fn next_seq(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

pub trait RemoteLocalApiAccessPolicy: Send + Sync {
    fn is_allowed(&self, method: &str, path: &str) -> bool;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAllRemoteLocalApiAccessPolicy;

impl RemoteLocalApiAccessPolicy for AllowAllRemoteLocalApiAccessPolicy {
    fn is_allowed(&self, _method: &str, _path: &str) -> bool {
        true
    }
}

pub fn validate_local_api_path(path: &str) -> Result<(), String> {
    if !path.starts_with('/') || path.starts_with("//") || path.contains("://") {
        return Err("path 必须是以 / 开头的本机 Local API 路径".to_string());
    }
    if !path.starts_with("/api/") {
        return Err("path 第一版只允许 /api/ 前缀".to_string());
    }
    Ok(())
}

pub fn build_local_api_url(addr: &str, path: &str) -> Result<reqwest::Url, String> {
    validate_local_api_path(path)?;
    reqwest::Url::parse(&format!("http://{addr}{path}"))
        .map_err(|error| format!("本机 Local API URL 构造失败：{error}"))
}

pub fn is_forwardable_header(name: &str) -> bool {
    !matches!(
        name.to_ascii_lowercase().as_str(),
        "connection" | "upgrade" | "host" | "content-length" | "transfer-encoding"
    )
}

pub async fn execute_local_api_request(
    addr: &str,
    params: LocalApiRequestParams,
    policy: &dyn RemoteLocalApiAccessPolicy,
) -> Result<LocalApiResponsePayload, String> {
    validate_local_api_path(&params.path)?;
    if !policy.is_allowed(&params.method, &params.path) {
        return Err("remote_api_forbidden".to_string());
    }

    let url = build_local_api_url(addr, &params.path)?;
    let method = reqwest::Method::from_bytes(params.method.as_bytes())
        .map_err(|error| format!("HTTP method 无效：{error}"))?;
    let client = reqwest::Client::new();
    let mut request = client.request(method, url);

    if let Some(headers) = params.headers {
        for (name, value) in headers {
            if is_forwardable_header(&name) {
                request = request.header(name, value);
            }
        }
    }
    if let Some(body) = params.body {
        request = request.json(&body);
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("本机 Local API 请求失败：{error}"))?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect();
    let text = response
        .text()
        .await
        .map_err(|error| format!("本机 Local API 响应读取失败：{error}"))?;
    let body = serde_json::from_str(&text).unwrap_or(Value::String(text));

    Ok(LocalApiResponsePayload {
        status,
        headers,
        body,
    })
}

pub fn parse_sse_event_block(block: &str) -> Result<ParsedSseEvent, String> {
    let mut id = None;
    let mut event = "message".to_string();
    let mut data_lines = Vec::new();

    for raw_line in block.lines() {
        let line = raw_line.trim_end_matches('\r');
        if let Some(value) = line.strip_prefix("id:") {
            id = Some(value.trim_start().to_string());
        } else if let Some(value) = line.strip_prefix("event:") {
            event = value.trim_start().to_string();
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_string());
        }
    }

    let data_text = data_lines.join("\n");
    let data = serde_json::from_str(&data_text).unwrap_or(Value::String(data_text));
    Ok(ParsedSseEvent { id, event, data })
}

pub fn stream_event_notification(stream_id: &str, seq: u64, event: ParsedSseEvent) -> Value {
    serde_json::json!({
        "version": 1,
        "type": "notification",
        "transport": {
            "kind": "relay"
        },
        "method": "local_api.stream.event",
        "params": {
            "stream_id": stream_id,
            "seq": seq,
            "event": event.event,
            "id": event.id,
            "data": event.data
        }
    })
}

pub fn stream_closed_notification(stream_id: &str, reason: impl Into<String>) -> Value {
    serde_json::json!({
        "version": 1,
        "type": "notification",
        "transport": {
            "kind": "relay"
        },
        "method": "local_api.stream.closed",
        "params": {
            "stream_id": stream_id,
            "reason": reason.into()
        }
    })
}

pub fn spawn_local_api_stream(
    addr: String,
    params: LocalApiRequestParams,
    policy: &dyn RemoteLocalApiAccessPolicy,
    notifications: UnboundedSender<Value>,
) -> Result<(String, tokio::task::JoinHandle<()>), String> {
    validate_local_api_path(&params.path)?;
    if !policy.is_allowed(&params.method, &params.path) {
        return Err("remote_api_forbidden".to_string());
    }

    let stream_id = format!("stream_{}", Uuid::new_v4());
    let stream_id_for_task = stream_id.clone();
    let handle = tokio::spawn(async move {
        let close_reason = match run_local_api_stream(
            addr,
            params,
            stream_id_for_task.clone(),
            notifications.clone(),
        )
        .await
        {
            Ok(()) => "closed".to_string(),
            Err(error) => error,
        };
        let _ = notifications.send(stream_closed_notification(
            &stream_id_for_task,
            close_reason,
        ));
    });

    Ok((stream_id, handle))
}

async fn run_local_api_stream(
    addr: String,
    params: LocalApiRequestParams,
    stream_id: String,
    notifications: UnboundedSender<Value>,
) -> Result<(), String> {
    let url = build_local_api_url(&addr, &params.path)?;
    let method = reqwest::Method::from_bytes(params.method.as_bytes())
        .map_err(|error| format!("HTTP method 无效：{error}"))?;
    let client = reqwest::Client::new();
    let mut request = client.request(method, url);

    if let Some(headers) = params.headers {
        for (name, value) in headers {
            if is_forwardable_header(&name) {
                request = request.header(name, value);
            }
        }
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("本机 Local API 流请求失败：{error}"))?;
    let mut chunks = response.bytes_stream();
    let mut buffer = String::new();
    let mut sequence = StreamNotificationSequence::default();

    while let Some(next) = chunks.next().await {
        let chunk = next.map_err(|error| format!("本机 Local API 流读取失败：{error}"))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(index) = find_sse_block_end(&buffer) {
            let block = buffer[..index].to_string();
            let drain_end = if buffer[index..].starts_with("\r\n\r\n") {
                index + 4
            } else {
                index + 2
            };
            buffer.drain(..drain_end);
            if block.trim().is_empty() {
                continue;
            }
            match parse_sse_event_block(&block) {
                Ok(event) => {
                    if notifications
                        .send(stream_event_notification(
                            &stream_id,
                            sequence.next_seq(),
                            event,
                        ))
                        .is_err()
                    {
                        return Ok(());
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }

    Ok(())
}

fn find_sse_block_end(buffer: &str) -> Option<usize> {
    match (buffer.find("\n\n"), buffer.find("\r\n\r\n")) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_http_like_request_params() {
        let params: LocalApiRequestParams = serde_json::from_value(json!({
            "method": "GET",
            "path": "/api/v1/session_project_groups?tool=codex",
            "headers": { "accept": "application/json" },
            "body": null
        }))
        .unwrap();

        assert_eq!(params.method, "GET");
        assert_eq!(params.path, "/api/v1/session_project_groups?tool=codex");
        assert_eq!(
            params.headers.unwrap().get("accept").unwrap(),
            "application/json"
        );
        assert!(params.body.is_none());
    }

    #[test]
    fn allow_all_policy_allows_every_relative_api_path() {
        let policy = AllowAllRemoteLocalApiAccessPolicy;

        assert!(policy.is_allowed("GET", "/api/v1/session_project_groups"));
        assert!(policy.is_allowed("POST", "/api/v1/session-control/send-instruction"));
    }

    #[test]
    fn validate_local_api_path_rejects_full_url_and_non_api_path() {
        assert!(validate_local_api_path("/api/v1/session_project_groups").is_ok());

        assert_eq!(
            validate_local_api_path("http://example.com/api/v1/session_project_groups")
                .unwrap_err(),
            "path 必须是以 / 开头的本机 Local API 路径"
        );
        assert_eq!(
            validate_local_api_path("/desktop-login").unwrap_err(),
            "path 第一版只允许 /api/ 前缀"
        );
    }

    #[test]
    fn builds_local_api_url_from_relative_path() {
        let url = build_local_api_url(
            "127.0.0.1:27874",
            "/api/v1/session_project_groups?tool=codex",
        )
        .unwrap();

        assert_eq!(
            url.as_str(),
            "http://127.0.0.1:27874/api/v1/session_project_groups?tool=codex"
        );
    }

    #[test]
    fn filters_hop_by_hop_headers() {
        assert!(is_forwardable_header("accept"));
        assert!(!is_forwardable_header("connection"));
        assert!(!is_forwardable_header("upgrade"));
    }

    #[test]
    fn parses_sse_event_block() {
        let event =
            parse_sse_event_block("id: 12\nevent: session_project_groups\ndata: {\"list\":[]}\n\n")
                .unwrap();

        assert_eq!(event.id.as_deref(), Some("12"));
        assert_eq!(event.event, "session_project_groups");
        assert_eq!(event.data, serde_json::json!({ "list": [] }));
    }

    #[test]
    fn stream_event_notification_uses_stable_shape() {
        let value = stream_event_notification(
            "stream_1",
            1,
            ParsedSseEvent {
                id: Some("12".to_string()),
                event: "session_project_groups".to_string(),
                data: serde_json::json!({ "list": [] }),
            },
        );

        assert_eq!(value["type"], "notification");
        assert_eq!(value["transport"]["kind"], "relay");
        assert_eq!(value["method"], "local_api.stream.event");
        assert_eq!(value["params"]["stream_id"], "stream_1");
        assert_eq!(value["params"]["seq"], 1);
        assert_eq!(value["params"]["event"], "session_project_groups");
    }

    #[test]
    fn stream_notification_sequence_increments_per_stream() {
        let mut sequence = StreamNotificationSequence::default();

        assert_eq!(sequence.next_seq(), 1);
        assert_eq!(sequence.next_seq(), 2);
    }
}
