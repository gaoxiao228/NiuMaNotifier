use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use niuma_api::tool_sessions::{ToolSessionProjectGroupsQuery, ToolSessionRegistry};
use niuma_core::remote::rpc_envelope::RemoteRpcEnvelope;
use niuma_core::store::NiumaStore;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

type LocalApiExecutor = Arc<
    dyn Fn(
            crate::remote::local_api_bridge::LocalApiRequestParams,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<
                            crate::remote::local_api_bridge::LocalApiResponsePayload,
                            String,
                        >,
                    > + Send,
            >,
        > + Send
        + Sync,
>;

pub fn handle_plain_rpc(request: Value) -> Result<Value, String> {
    handle_plain_rpc_inner(request, None)
}

#[derive(Clone)]
pub struct RemoteRpcContext {
    store: NiumaStore,
    tool_sessions: ToolSessionRegistry,
    local_api_executor: LocalApiExecutor,
    stream_handles: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    notification_sender: Option<UnboundedSender<Value>>,
}

impl RemoteRpcContext {
    pub fn new(store: NiumaStore, tool_sessions: ToolSessionRegistry) -> Self {
        let executor: LocalApiExecutor = Arc::new(|params| {
            Box::pin(async move {
                crate::remote::local_api_bridge::execute_local_api_request(
                    &niuma_api::local_api_addr(),
                    params,
                    &crate::remote::local_api_bridge::AllowAllRemoteLocalApiAccessPolicy,
                )
                .await
            })
        });
        Self {
            store,
            tool_sessions,
            local_api_executor: executor,
            stream_handles: Arc::new(Mutex::new(HashMap::new())),
            notification_sender: None,
        }
    }

    pub fn with_notification_sender(mut self, sender: UnboundedSender<Value>) -> Self {
        self.notification_sender = Some(sender);
        self
    }

    #[cfg(test)]
    pub fn new_for_test_with_local_api_executor(
        store: NiumaStore,
        tool_sessions: ToolSessionRegistry,
        local_api_executor: LocalApiExecutor,
    ) -> Self {
        Self {
            store,
            tool_sessions,
            local_api_executor,
            stream_handles: Arc::new(Mutex::new(HashMap::new())),
            notification_sender: None,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct SessionProjectGroupsParams {
    tool: Option<String>,
    project_path: Option<String>,
    include_subagents: Option<bool>,
    page: Option<usize>,
    page_size: Option<usize>,
}

pub fn handle_plain_rpc_with_context(
    request: Value,
    context: &RemoteRpcContext,
) -> Result<Value, String> {
    handle_plain_rpc_inner(request, Some(context))
}

pub async fn handle_plain_rpc_with_context_async(
    request: Value,
    context: &RemoteRpcContext,
) -> Result<Value, String> {
    let envelope: RemoteRpcEnvelope = serde_json::from_value(request.clone())
        .map_err(|error| format!("RPC request envelope 校验失败：{error}"))?;

    let RemoteRpcEnvelope::Request {
        version,
        id,
        method,
        params,
    } = envelope
    else {
        return Err("RPC envelope 必须是 request 类型".to_string());
    };

    if version != 1 {
        return Err(format!("不支持的 RPC envelope version：{version}"));
    }

    match method.as_str() {
        "local_api.request" => handle_local_api_request(id, params, context).await,
        "local_api.stream" => handle_local_api_stream(id, params, context),
        "local_api.stream.close" => handle_local_api_stream_close(id, params, context),
        _ => handle_plain_rpc_with_context(request, context),
    }
}

fn handle_plain_rpc_inner(
    request: Value,
    context: Option<&RemoteRpcContext>,
) -> Result<Value, String> {
    let envelope: RemoteRpcEnvelope = serde_json::from_value(request)
        .map_err(|error| format!("RPC request envelope 校验失败：{error}"))?;

    let RemoteRpcEnvelope::Request {
        version,
        id,
        method,
        params,
    } = envelope
    else {
        return Err("RPC envelope 必须是 request 类型".to_string());
    };

    if version != 1 {
        return Err(format!("不支持的 RPC envelope version：{version}"));
    }

    let result = match method.as_str() {
        "rpc.ping" => json!({ "pong": true }),
        // Task 7 只返回占位状态，避免绕过 MainState 架构直接读取。
        "state.get" => json!({ "state": "unknown", "source": "remote_mvp" }),
        "session.project_groups" => {
            let Some(context) = context else {
                // session 查询依赖宿主进程注入真实 store/registry，不能用空上下文伪造成功结果。
                return Ok(error_response(
                    id,
                    "context_unavailable",
                    "session.project_groups 需要 RemoteRpcContext",
                ));
            };
            return handle_session_project_groups(id, params, context);
        }
        "local_api.request" => {
            let params: crate::remote::local_api_bridge::LocalApiRequestParams =
                match serde_json::from_value(params) {
                    Ok(params) => params,
                    Err(error) => {
                        return Ok(error_response(
                            id,
                            "invalid_params",
                            format!("local_api.request 参数错误：{error}"),
                        ));
                    }
                };
            if let Err(error) =
                crate::remote::local_api_bridge::validate_local_api_path(&params.path)
            {
                return Ok(error_response(id, "invalid_path", error));
            }
            return Ok(error_response(
                id,
                "not_implemented",
                "local_api.request 需要异步 RPC 入口",
            ));
        }
        _ => {
            return Ok(error_response(
                id,
                "method_not_found",
                format!("unknown RPC method: {method}"),
            ));
        }
    };

    Ok(success_response(id, result))
}

async fn handle_local_api_request(
    id: String,
    params: Value,
    context: &RemoteRpcContext,
) -> Result<Value, String> {
    let params: crate::remote::local_api_bridge::LocalApiRequestParams =
        match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => {
                return Ok(error_response(
                    id,
                    "invalid_params",
                    format!("local_api.request 参数错误：{error}"),
                ));
            }
        };
    if let Err(error) = crate::remote::local_api_bridge::validate_local_api_path(&params.path) {
        return Ok(error_response(id, "invalid_path", error));
    }

    match (context.local_api_executor)(params).await {
        Ok(payload) => Ok(success_response(id, json!(payload))),
        Err(error) => Ok(error_response(id, "local_api_request_failed", error)),
    }
}

fn handle_local_api_stream(
    id: String,
    params: Value,
    context: &RemoteRpcContext,
) -> Result<Value, String> {
    let params: crate::remote::local_api_bridge::LocalApiRequestParams =
        match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => {
                return Ok(error_response(
                    id,
                    "invalid_params",
                    format!("local_api.stream 参数错误：{error}"),
                ));
            }
        };
    if let Err(error) = crate::remote::local_api_bridge::validate_local_api_path(&params.path) {
        return Ok(error_response(id, "invalid_path", error));
    }
    let Some(sender) = context.notification_sender.clone() else {
        return Ok(error_response(
            id,
            "notification_unavailable",
            "local_api.stream 需要 relay 通知通道",
        ));
    };

    match crate::remote::local_api_bridge::spawn_local_api_stream(
        niuma_api::local_api_addr(),
        params,
        &crate::remote::local_api_bridge::AllowAllRemoteLocalApiAccessPolicy,
        sender,
    ) {
        Ok((stream_id, handle)) => {
            let mut handles = context
                .stream_handles
                .lock()
                .map_err(|_| "local_api stream 任务表锁定失败".to_string())?;
            handles.insert(stream_id.clone(), handle);
            Ok(success_response(id, json!({ "stream_id": stream_id })))
        }
        Err(error) => Ok(error_response(id, "local_api_stream_failed", error)),
    }
}

fn handle_local_api_stream_close(
    id: String,
    params: Value,
    context: &RemoteRpcContext,
) -> Result<Value, String> {
    let params: crate::remote::local_api_bridge::LocalApiStreamCloseParams =
        match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => {
                return Ok(error_response(
                    id,
                    "invalid_params",
                    format!("local_api.stream.close 参数错误：{error}"),
                ));
            }
        };
    let mut handles = context
        .stream_handles
        .lock()
        .map_err(|_| "local_api stream 任务表锁定失败".to_string())?;
    if let Some(handle) = handles.remove(&params.stream_id) {
        handle.abort();
    }
    Ok(success_response(id, json!({ "closed": true })))
}

fn handle_session_project_groups(
    id: String,
    params: Value,
    context: &RemoteRpcContext,
) -> Result<Value, String> {
    let params: SessionProjectGroupsParams = match serde_json::from_value(params) {
        Ok(params) => params,
        Err(error) => {
            return Ok(error_response(
                id,
                "business_validation",
                format!("session.project_groups 参数错误：{error}"),
            ));
        }
    };
    let query = ToolSessionProjectGroupsQuery {
        tool: trim_optional(params.tool),
        project_path: trim_optional(params.project_path),
        include_subagents: params.include_subagents.unwrap_or(false),
        page: params.page,
        page_size: params.page_size,
    };

    // RPC 与本机 HTTP/SSE 查询共用同一份运行时状态叠加逻辑，避免远程只读视图漂移。
    let runtime_states = match context.store.runtime_state_list() {
        Ok(runtime_states) => runtime_states,
        Err(error) => return Ok(error_response(id, "system", error)),
    };
    match context
        .tool_sessions
        .project_groups_with_runtime(query, &runtime_states)
    {
        Ok(page) => Ok(success_response(id, json!(page))),
        Err(error) => Ok(error_response(id, "business_validation", error)),
    }
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn success_response(id: String, result: Value) -> Value {
    json!({
        "version": 1,
        "type": "response",
        "id": id,
        "ok": true,
        "result": result
    })
}

fn error_response(id: String, code: &str, message: impl Into<String>) -> Value {
    json!({
        "version": 1,
        "type": "response",
        "id": id,
        "ok": false,
        "error": {
            "code": code,
            "message": message.into()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use niuma_api::tool_sessions::ToolSessionRegistry;
    use niuma_core::models::{EventType, NiumaEvent, ToolKind};
    use niuma_core::store::NiumaStore;
    use niuma_core::tool_session::{
        ToolSessionListItem, ToolSessionNormalizationStatus, ToolSessionScope, ToolSessionStatus,
    };
    use serde_json::json;

    #[test]
    fn handles_rpc_ping() {
        let response = handle_plain_rpc(json!({
            "version": 1,
            "type": "request",
            "id": "req_1",
            "method": "rpc.ping",
            "params": {}
        }))
        .unwrap();

        assert_eq!(
            response,
            json!({
                "version": 1,
                "type": "response",
                "id": "req_1",
                "ok": true,
                "result": { "pong": true }
            })
        );
    }

    #[test]
    fn handles_state_get_with_remote_mvp_placeholder() {
        let response = handle_plain_rpc(json!({
            "version": 1,
            "type": "request",
            "id": "req_2",
            "method": "state.get",
            "params": {}
        }))
        .unwrap();

        assert_eq!(response["ok"], true);
        assert_eq!(
            response["result"],
            json!({ "state": "unknown", "source": "remote_mvp" })
        );
    }

    #[test]
    fn session_project_groups_without_context_returns_context_unavailable() {
        let response = handle_plain_rpc(json!({
            "version": 1,
            "type": "request",
            "id": "req_no_context",
            "method": "session.project_groups",
            "params": { "page": 1, "page_size": 10 }
        }))
        .unwrap();

        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "context_unavailable");
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("RemoteRpcContext"));
    }

    #[test]
    fn handles_session_project_groups_with_runtime_overlay() {
        let store = test_store("rpc_session_project_groups_runtime_overlay");
        let registry = ToolSessionRegistry::new();
        registry.replace_snapshot(
            ToolKind::Codex,
            vec![tool_session_item(
                "session-1",
                ToolKind::Codex,
                30,
                20,
                true,
                false,
            )],
        );
        store
            .append_event(sample_event("event-1", "session-1"))
            .unwrap();
        let context = RemoteRpcContext::new(store, registry);

        let response = handle_plain_rpc_with_context(
            json!({
                "version": 1,
                "type": "request",
                "id": "req_3",
                "method": "session.project_groups",
                "params": {
                    "tool": "codex",
                    "page": 1,
                    "page_size": 10
                }
            }),
            &context,
        )
        .unwrap();

        assert_eq!(response["ok"], true);
        assert_eq!(response["result"]["page"], 1);
        assert_eq!(response["result"]["page_size"], 10);
        assert_eq!(response["result"]["total"], 1);
        assert_eq!(response["result"]["list"][0]["project_path"], "/tmp/demo");
        assert_eq!(
            response["result"]["list"][0]["sessions"][0]["primary_session_id"],
            "session-1"
        );
        assert_eq!(
            response["result"]["list"][0]["sessions"][0]["runtime_status"],
            "waiting_approval"
        );
        assert_eq!(
            response["result"]["list"][0]["sessions"][0]["runtime_last_event_id"],
            "event-1"
        );
    }

    #[test]
    fn session_project_groups_returns_business_error_response_for_invalid_page() {
        let context = RemoteRpcContext::new(
            test_store("rpc_session_project_groups_invalid_page"),
            ToolSessionRegistry::new(),
        );

        let response = handle_plain_rpc_with_context(
            json!({
                "version": 1,
                "type": "request",
                "id": "req_invalid_page",
                "method": "session.project_groups",
                "params": { "page": 0 }
            }),
            &context,
        )
        .unwrap();

        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "business_validation");
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("page"));
    }

    #[test]
    fn local_api_request_returns_http_like_response() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let context = RemoteRpcContext::new_for_test_with_local_api_executor(
                test_store("rpc_local_api_request"),
                ToolSessionRegistry::new(),
                std::sync::Arc::new(|_params| {
                    Box::pin(async move {
                        Ok(crate::remote::local_api_bridge::LocalApiResponsePayload {
                            status: 200,
                            headers: std::collections::BTreeMap::new(),
                            body: serde_json::json!({
                                "code": 0,
                                "message": "ok",
                                "data": { "value": 1 }
                            }),
                        })
                    })
                }),
            );

            let response = handle_plain_rpc_with_context_async(
                json!({
                    "version": 1,
                    "type": "request",
                    "id": "req_local",
                    "method": "local_api.request",
                    "params": {
                        "method": "GET",
                        "path": "/api/v1/demo",
                        "body": null
                    }
                }),
                &context,
            )
            .await
            .unwrap();

            assert_eq!(response["ok"], true);
            assert_eq!(response["result"]["status"], 200);
            assert_eq!(response["result"]["body"]["data"]["value"], 1);
        });
    }

    #[test]
    fn local_api_stream_requires_notification_channel() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let context = RemoteRpcContext::new(
                test_store("rpc_local_api_stream_without_notifications"),
                ToolSessionRegistry::new(),
            );

            let response = handle_plain_rpc_with_context_async(
                json!({
                    "version": 1,
                    "type": "request",
                    "id": "req_stream",
                    "method": "local_api.stream",
                    "params": {
                        "method": "GET",
                        "path": "/api/v1/session_project_groups/stream?tool=codex",
                        "body": null
                    }
                }),
                &context,
            )
            .await
            .unwrap();

            assert_eq!(response["ok"], false);
            assert_eq!(response["error"]["code"], "notification_unavailable");
        });
    }

    #[test]
    fn returns_method_not_found_for_legacy_session_list() {
        let response = handle_plain_rpc(json!({
            "version": 1,
            "type": "request",
            "id": "req_3",
            "method": "session.list",
            "params": {}
        }))
        .unwrap();

        assert_eq!(
            response,
            json!({
                "version": 1,
                "type": "response",
                "id": "req_3",
                "ok": false,
                "error": {
                    "code": "method_not_found",
                    "message": "unknown RPC method: session.list"
                }
            })
        );
    }

    #[test]
    fn returns_method_not_found_for_unknown_method() {
        let response = handle_plain_rpc(json!({
            "version": 1,
            "type": "request",
            "id": "req_4",
            "method": "unknown.method",
            "params": {}
        }))
        .unwrap();

        assert_eq!(
            response,
            json!({
                "version": 1,
                "type": "response",
                "id": "req_4",
                "ok": false,
                "error": {
                    "code": "method_not_found",
                    "message": "unknown RPC method: unknown.method"
                }
            })
        );
    }

    #[test]
    fn rejects_request_without_id_or_method() {
        let missing_id = handle_plain_rpc(json!({
            "version": 1,
            "type": "request",
            "method": "rpc.ping",
            "params": {}
        }))
        .unwrap_err();
        let missing_method = handle_plain_rpc(json!({
            "version": 1,
            "type": "request",
            "id": "req_5",
            "params": {}
        }))
        .unwrap_err();

        assert!(missing_id.contains("id"));
        assert!(missing_method.contains("method"));
    }

    fn test_store(name: &str) -> NiumaStore {
        let dir = std::env::temp_dir().join(format!(
            "niuma-desktop-rpc-router-{name}-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        NiumaStore::new(dir.join("niuma.sqlite"))
    }

    fn tool_session_item(
        session_id: &str,
        tool: ToolKind,
        last_seen_at: i64,
        modified_at: i64,
        is_active: bool,
        is_subagent: bool,
    ) -> ToolSessionListItem {
        let tool_id = tool.as_str().to_string();
        ToolSessionListItem {
            id: format!("{tool_id}:{session_id}"),
            tool,
            session_id: session_id.to_string(),
            project_path: "/tmp/demo".to_string(),
            project_name: "demo".to_string(),
            file_path: format!("/tmp/demo/{session_id}.jsonl"),
            modified_at: Utc.timestamp_opt(modified_at, 0).single().unwrap(),
            discovered_at: Utc.timestamp_opt(1, 0).single().unwrap(),
            last_seen_at: Utc.timestamp_opt(last_seen_at, 0).single().unwrap(),
            is_active,
            is_subagent,
            parent_session_id: is_subagent.then(|| "parent-session".to_string()),
            normalized_session_id: Some(if is_subagent {
                "parent-session".to_string()
            } else {
                session_id.to_string()
            }),
            session_scope: Some(if is_subagent {
                ToolSessionScope::Subagent
            } else {
                ToolSessionScope::Main
            }),
            agent_nickname: None,
            agent_role: None,
            normalization_status: Some(ToolSessionNormalizationStatus::Resolved),
            first_user_message_preview: None,
            first_user_message_at: None,
            control: None,
            status: if is_active {
                ToolSessionStatus::Active
            } else {
                ToolSessionStatus::Inactive
            },
        }
    }

    fn sample_event(id: &str, session_id: &str) -> NiumaEvent {
        NiumaEvent {
            id: id.to_string(),
            dedupe_key: format!("{id}-dedupe"),
            source: "test".to_string(),
            tool: ToolKind::Codex,
            session_id: session_id.to_string(),
            parent_session_id: None,
            normalized_session_id: None,
            session_scope: None,
            agent_nickname: None,
            agent_role: None,
            project_path: "/tmp/demo".to_string(),
            project_name: "demo".to_string(),
            event_type: EventType::ApprovalRequested,
            severity: "urgent".to_string(),
            summary: "Bash: cargo test".to_string(),
            content: None,
            error_message: None,
            attention_resolve_key: None,
            completion_reason: None,
            failure_reason: None,
            payload_ref: None,
            interaction: None,
            created_at: Utc.timestamp_opt(1_000, 0).single().unwrap(),
        }
    }
}
