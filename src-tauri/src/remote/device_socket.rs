use futures_util::{SinkExt, StreamExt};
use http::header::AUTHORIZATION;
use niuma_api::tool_sessions::ToolSessionRegistry;
use niuma_core::remote::config::RemoteConfig;
use niuma_core::remote::connection_policy::{
    classify_device_socket_close, device_socket_url, DeviceSocketCloseReason,
};
use niuma_core::remote::signaling::ConnectionInvite;
use niuma_core::remote::signaling::{parse_device_signal_message, DeviceSignalMessage};
use niuma_core::store::NiumaStore;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

#[derive(Clone)]
pub struct DeviceSocketConnectRequest {
    pub server_url: String,
    pub device_id: String,
    pub device_token: String,
    pub heartbeat_interval_seconds: u64,
    pub remote_config: RemoteConfig,
    // relay plain RPC 必须使用宿主进程真实状态上下文，避免回退到 Local API。
    pub store: NiumaStore,
    pub tool_sessions: ToolSessionRegistry,
}

impl DeviceSocketConnectRequest {
    pub fn socket_url(&self) -> Result<String, String> {
        device_socket_url(&self.server_url)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceSocketRunResult {
    Closed(DeviceSocketCloseReason),
    Failed(String),
}

#[derive(Clone, PartialEq, Eq)]
pub struct DeviceRelayStartRequest {
    pub server_url: String,
    pub connection_id: String,
    pub connection_token: String,
    pub device_token: String,
}

type RelayTaskMap = HashMap<String, JoinHandle<()>>;

fn abort_relay_for_connection(relay_tasks: &mut RelayTaskMap, connection_id: &str) {
    if let Some(handle) = relay_tasks.remove(connection_id) {
        handle.abort();
    }
}

fn abort_all_relays(relay_tasks: &mut RelayTaskMap) {
    for (_, handle) in relay_tasks.drain() {
        handle.abort();
    }
}

fn replace_relay_task(
    relay_tasks: &mut RelayTaskMap,
    connection_id: String,
    start_task: impl FnOnce() -> JoinHandle<()>,
) {
    // 桌面端当前只允许一个有效远程连接，新 relay 启动前必须终止旧 relay。
    abort_all_relays(relay_tasks);
    relay_tasks.insert(connection_id, start_task());
}

fn apply_connection_invite_relay_lifecycle(
    relay_tasks: &mut RelayTaskMap,
    config: &RemoteConfig,
    device_token: &str,
    invite: &ConnectionInvite,
    outbound: &[Value],
    start_relay: impl FnOnce(DeviceRelayStartRequest) -> JoinHandle<()>,
) {
    let Some(transport) = connection_accept_transport(outbound, &invite.connection_id) else {
        return;
    };
    if transport != "relay" {
        // 同一连接升级出 WebRTC 时 relay 作为热备保留；不同连接仍替换旧 relay。
        if !relay_tasks.contains_key(&invite.connection_id) {
            abort_all_relays(relay_tasks);
        }
        return;
    }

    let Some(relay_request) =
        relay_start_request_after_accept(config, device_token, invite, outbound)
    else {
        abort_all_relays(relay_tasks);
        return;
    };
    let connection_id = relay_request.connection_id.clone();
    replace_relay_task(relay_tasks, connection_id, || start_relay(relay_request));
}

fn connection_accept_transport<'a>(outbound: &'a [Value], connection_id: &str) -> Option<&'a str> {
    outbound.iter().find_map(|message| {
        if message.get("type").and_then(Value::as_str) != Some("connection.accept") {
            return None;
        }
        let data = message.get("data")?;
        let accepted_connection_id = data.get("connection_id").and_then(Value::as_str)?;
        if accepted_connection_id != connection_id {
            return None;
        }
        data.get("transport").and_then(Value::as_str)
    })
}

pub fn device_authorization_header(device_token: &str) -> String {
    format!("Device {device_token}")
}

pub fn build_device_socket_upgrade_request(
    socket_url: &str,
    device_token: &str,
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, String> {
    let mut request = socket_url
        .into_client_request()
        .map_err(|error| format!("构造远程 WebSocket 握手请求失败：{error}"))?;
    request.headers_mut().insert(
        AUTHORIZATION,
        device_authorization_header(device_token)
            .parse()
            .map_err(|error| format!("构造远程授权头失败：{error}"))?,
    );
    Ok(request)
}

pub async fn run_device_socket_once(
    request: DeviceSocketConnectRequest,
    signaling_manager: crate::remote::signaling::RemoteSignalingManager,
    webrtc_config: crate::remote::webrtc_transport::WebRtcTransportConfig,
) -> DeviceSocketRunResult {
    let socket_url = match request.socket_url() {
        Ok(value) => value,
        Err(error) => return DeviceSocketRunResult::Failed(error),
    };
    let upgrade_request =
        match build_device_socket_upgrade_request(&socket_url, &request.device_token) {
            Ok(value) => value,
            Err(error) => {
                return DeviceSocketRunResult::Failed(format!("构造远程连接请求失败：{error}"));
            }
        };
    let (stream, _) = match connect_async(upgrade_request).await {
        Ok(value) => value,
        Err(error) => return DeviceSocketRunResult::Failed(format!("远程设备连接失败：{error}")),
    };
    let (mut writer, mut reader) = stream.split();
    if let Err(error) = writer
        .send(Message::Text(
            device_hello_message(&request.device_id).to_string(),
        ))
        .await
    {
        return DeviceSocketRunResult::Failed(format!("发送远程 hello 失败：{error}"));
    }
    signaling_manager.mark_device_online();

    let mut heartbeat = time::interval(Duration::from_secs(request.heartbeat_interval_seconds));
    let mut relay_tasks = RelayTaskMap::default();
    let (signal_outbound_tx, mut signal_outbound_rx) =
        tokio::sync::mpsc::unbounded_channel::<Value>();
    loop {
        tokio::select! {
            outbound = signal_outbound_rx.recv() => {
                let Some(message) = outbound else {
                    continue;
                };
                if let Err(error) = writer.send(Message::Text(message.to_string())).await {
                    abort_all_relays(&mut relay_tasks);
                    return DeviceSocketRunResult::Failed(format!("发送远程异步信令失败：{error}"));
                }
            }
            _ = heartbeat.tick() => {
                if let Err(error) = writer
                    .send(Message::Text(device_heartbeat_message().to_string()))
                    .await
                {
                    abort_all_relays(&mut relay_tasks);
                    return DeviceSocketRunResult::Failed(format!("发送远程 heartbeat 失败：{error}"));
                }
            }
            next = reader.next() => {
                match next {
                    Some(Ok(Message::Close(frame))) => {
                        abort_all_relays(&mut relay_tasks);
                        return DeviceSocketRunResult::Closed(classify_device_socket_close(
                            frame.map(|value| value.code.into())
                        ));
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if let Err(error) = writer.send(Message::Pong(payload)).await {
                            abort_all_relays(&mut relay_tasks);
                            return DeviceSocketRunResult::Failed(format!("回复远程 ping 失败：{error}"));
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        let message = match parse_device_text_message(text) {
                            Ok(Some(value)) => value,
                            Ok(None) => continue,
                            Err(error) => {
                                abort_all_relays(&mut relay_tasks);
                                return DeviceSocketRunResult::Failed(error);
                            }
                        };
                        let outbound = match message {
                            DeviceSignalMessage::ConnectionInvite { data, .. } => {
                                let outbound = signaling_manager.handle_invite(&request.remote_config, data.clone());
                                let store = request.store.clone();
                                let tool_sessions = request.tool_sessions.clone();
                                let relay_signaling_manager = signaling_manager.clone();
                                apply_connection_invite_relay_lifecycle(
                                    &mut relay_tasks,
                                    &request.remote_config,
                                    &request.device_token,
                                    &data,
                                    &outbound,
                                    move |relay_request| {
                                        let rpc_context = crate::remote::rpc_router::RemoteRpcContext::new(
                                            store,
                                            tool_sessions,
                                        );
                                        tokio::spawn(async move {
                                            let connection_id = relay_request.connection_id.clone();
                                            if let Err(error) = crate::remote::relay_transport::run_device_relay_once(
                                                &relay_request.server_url,
                                                &connection_id,
                                                &relay_request.connection_token,
                                                &relay_request.device_token,
                                                rpc_context,
                                            ).await {
                                                eprintln!("NiumaNotifier remote relay stopped: {error}");
                                            }
                                            // relay 传输层结束后必须释放 signaling session；
                                            // 否则设备端会继续认为旧连接占用中，后续客户端收到 busy。
                                            relay_signaling_manager.clear_session(&connection_id);
                                        })
                                    },
                                );
                                outbound
                            }
                            DeviceSignalMessage::SignalOffer { data, .. } => {
                                let rpc_context = crate::remote::rpc_router::RemoteRpcContext::new(
                                    request.store.clone(),
                                    request.tool_sessions.clone(),
                                );
                                signaling_manager
                                    .handle_offer_async(
                                        &request.remote_config,
                                        data,
                                        webrtc_config.clone(),
                                        rpc_context,
                                        Some(signal_outbound_tx.clone()),
                                    )
                                    .await
                            }
                            DeviceSignalMessage::SignalIceCandidate { data, .. } => {
                                signaling_manager.handle_ice_candidate_async(data).await
                            }
                            DeviceSignalMessage::SignalCancel { id, data } => {
                                abort_relay_for_connection(&mut relay_tasks, &data.connection_id);
                                signaling_manager.handle_message(
                                    &request.remote_config,
                                    DeviceSignalMessage::SignalCancel { id, data },
                                )
                            }
                            other => signaling_manager.handle_message(&request.remote_config, other),
                        };
                        for message in outbound {
                            if let Err(error) = writer.send(Message::Text(message.to_string())).await {
                                abort_all_relays(&mut relay_tasks);
                                return DeviceSocketRunResult::Failed(format!("发送远程信令响应失败：{error}"));
                            }
                        }
                    }
                    Some(Ok(_message)) => {}
                    Some(Err(error)) => {
                        abort_all_relays(&mut relay_tasks);
                        return DeviceSocketRunResult::Failed(format!("读取远程设备连接失败：{error}"));
                    }
                    None => {
                        abort_all_relays(&mut relay_tasks);
                        return DeviceSocketRunResult::Closed(DeviceSocketCloseReason::NetworkError);
                    }
                }
            }
        }
    }
}

pub fn relay_start_request_after_accept(
    config: &RemoteConfig,
    device_token: &str,
    invite: &ConnectionInvite,
    outbound: &[Value],
) -> Option<DeviceRelayStartRequest> {
    if connection_accept_transport(outbound, &invite.connection_id) != Some("relay") {
        return None;
    }

    Some(DeviceRelayStartRequest {
        server_url: config.server_url.clone(),
        connection_id: invite.connection_id.clone(),
        connection_token: invite.connection_token.clone()?,
        device_token: device_token.to_string(),
    })
}

pub fn parse_device_text_message(text: String) -> Result<Option<DeviceSignalMessage>, String> {
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("远程设备消息 JSON 解析失败：{error}"))?;
    match parse_device_signal_message(value) {
        Ok(message) => Ok(Some(message)),
        Err(error) => {
            // 非信令文本消息留给后续协议扩展；当前连接循环只消费已知信令。
            if error.starts_with("未知远程信令消息类型") {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}

pub fn dispatch_device_text_message(
    config: &RemoteConfig,
    text: String,
    mut handler: impl FnMut(&RemoteConfig, DeviceSignalMessage) -> Vec<Value>,
) -> Result<Vec<Value>, String> {
    Ok(parse_device_text_message(text)?
        .map(|message| handler(config, message))
        .unwrap_or_default())
}

pub fn is_async_webrtc_offer(message: &DeviceSignalMessage) -> bool {
    matches!(message, DeviceSignalMessage::SignalOffer { .. })
}

pub fn device_hello_message(device_id: &str) -> Value {
    json!({
        "version": 1,
        "type": "device.hello",
        "id": format!("msg_{}", Uuid::new_v4()),
        "data": {
            "device_id": device_id,
            "agent_protocol_version": 1,
            "rpc_protocol_version": 1,
            "capabilities": {
                "supports_webrtc": true,
                "supports_relay": true,
                "supports_remote_control": true
            }
        }
    })
}

pub fn device_heartbeat_message() -> Value {
    json!({
        "version": 1,
        "type": "device.heartbeat",
        "id": format!("msg_{}", Uuid::new_v4()),
        "data": {}
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_device_hello_message() {
        let message = device_hello_message("dev_1");
        assert_eq!(message["type"], "device.hello");
        assert_eq!(message["data"]["device_id"], "dev_1");
        assert_eq!(message["data"]["agent_protocol_version"], 1);
    }

    #[test]
    fn builds_heartbeat_message() {
        let message = device_heartbeat_message();
        assert_eq!(message["type"], "device.heartbeat");
    }
}

#[cfg(test)]
mod connection_tests {
    use super::*;

    #[test]
    fn builds_device_authorization_header() {
        assert_eq!(
            device_authorization_header("dvt_secret"),
            "Device dvt_secret"
        );
    }

    #[test]
    fn builds_websocket_upgrade_request_with_required_headers() {
        let request =
            build_device_socket_upgrade_request("ws://127.0.0.1:27880/ws/device", "dvt_secret")
                .unwrap();

        assert_eq!(
            request.headers().get("authorization").unwrap(),
            "Device dvt_secret"
        );
        assert!(request.headers().contains_key("sec-websocket-key"));
        assert_eq!(request.headers().get("upgrade").unwrap(), "websocket");
    }

    #[test]
    fn token_is_not_embedded_in_url() {
        let request = DeviceSocketConnectRequest {
            server_url: "https://remote.example.com".to_string(),
            device_id: "dev_1".to_string(),
            device_token: "dvt_secret".to_string(),
            heartbeat_interval_seconds: 20,
            remote_config: niuma_core::remote::config::RemoteConfig::default_for_server(
                "https://remote.example.com",
            ),
            store: test_store("device-socket-url"),
            tool_sessions: ToolSessionRegistry::new(),
        };

        assert_eq!(
            request.socket_url().unwrap(),
            "wss://remote.example.com/ws/device"
        );
        assert!(!request.socket_url().unwrap().contains("dvt_secret"));
    }

    fn test_store(name: &str) -> NiumaStore {
        let path = std::env::temp_dir().join(format!("{name}-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        NiumaStore::new(path)
    }
}

#[cfg(test)]
mod signaling_dispatch_tests {
    use super::*;
    use niuma_core::remote::config::RemoteConfig;
    use serde_json::json;
    use tokio::sync::oneshot;

    #[test]
    fn dispatches_connection_invite_to_handler() {
        let config = RemoteConfig::default_for_server("https://remote.example.com");
        let outbound = dispatch_device_text_message(
            &config,
            json!({
                "version": 1,
                "type": "connection.invite",
                "id": "msg_1",
                "data": {
                    "connection_id": "conn_1",
                    "connection_token": "cnt_relay_secret",
                    "client_id": "web_1",
                    "client_label": "Chrome",
                    "transport_preference": "webrtc",
                    "expires_at": "2026-06-28T00:02:00.000Z"
                }
            })
            .to_string(),
            |_, message| {
                assert_eq!(message.connection_id(), "conn_1");
                vec![serde_json::json!({
                    "version": 1,
                    "type": "connection.accept",
                    "id": "msg_2",
                    "data": { "connection_id": "conn_1", "transport": "webrtc" }
                })]
            },
        );

        assert_eq!(outbound.unwrap()[0]["type"], "connection.accept");
    }

    #[test]
    fn builds_relay_start_request_after_relay_accept() {
        let config = RemoteConfig::default_for_server("http://127.0.0.1:27880");
        let invite = niuma_core::remote::signaling::ConnectionInvite {
            connection_id: "conn_1".to_string(),
            connection_token: Some("cnt_relay_secret".to_string()),
            client_id: "web_1".to_string(),
            client_label: None,
            transport_preference: niuma_core::remote::signaling::TransportPreference::Relay,
            expires_at: "2026-06-28T00:02:00.000Z".to_string(),
        };
        let outbound = vec![serde_json::json!({
            "version": 1,
            "type": "connection.accept",
            "id": "msg_1",
            "data": {
                "connection_id": "conn_1",
                "transport": "relay"
            }
        })];

        let request = relay_start_request_after_accept(&config, "dvt_secret", &invite, &outbound);

        let request = request.unwrap();
        assert_eq!(request.server_url, "http://127.0.0.1:27880");
        assert_eq!(request.connection_id, "conn_1");
        assert_eq!(request.connection_token, "cnt_relay_secret");
        assert_eq!(request.device_token, "dvt_secret");
    }

    #[test]
    fn skips_relay_start_without_relay_accept_or_token() {
        let config = RemoteConfig::default_for_server("http://127.0.0.1:27880");
        let mut invite = niuma_core::remote::signaling::ConnectionInvite {
            connection_id: "conn_1".to_string(),
            connection_token: Some("cnt_relay_secret".to_string()),
            client_id: "web_1".to_string(),
            client_label: None,
            transport_preference: niuma_core::remote::signaling::TransportPreference::Relay,
            expires_at: "2026-06-28T00:02:00.000Z".to_string(),
        };
        let webrtc_accept = vec![serde_json::json!({
            "type": "connection.accept",
            "data": { "connection_id": "conn_1", "transport": "webrtc" }
        })];

        assert!(
            relay_start_request_after_accept(&config, "dvt_secret", &invite, &webrtc_accept)
                .is_none()
        );

        invite.connection_token = None;
        let relay_accept = vec![serde_json::json!({
            "type": "connection.accept",
            "data": { "connection_id": "conn_1", "transport": "relay" }
        })];
        assert!(
            relay_start_request_after_accept(&config, "dvt_secret", &invite, &relay_accept)
                .is_none()
        );
    }

    #[tokio::test]
    async fn abort_relay_for_connection_removes_and_aborts_matching_task() {
        let mut tasks = RelayTaskMap::default();
        let (cancel_tx, cancel_rx) = oneshot::channel();
        tasks.insert("conn_1".to_string(), cancellable_relay_task(cancel_tx));
        tokio::task::yield_now().await;

        abort_relay_for_connection(&mut tasks, "conn_1");

        assert!(tasks.is_empty());
        cancel_rx.await.unwrap();
    }

    #[tokio::test]
    async fn replace_relay_task_aborts_existing_tasks_and_keeps_new_connection() {
        let mut tasks = RelayTaskMap::default();
        let (old_tx, old_rx) = oneshot::channel();
        let (new_tx, _new_rx) = oneshot::channel();
        tasks.insert("conn_old".to_string(), cancellable_relay_task(old_tx));
        tokio::task::yield_now().await;

        replace_relay_task(&mut tasks, "conn_new".to_string(), || {
            cancellable_relay_task(new_tx)
        });

        assert!(tasks.contains_key("conn_new"));
        assert!(!tasks.contains_key("conn_old"));
        old_rx.await.unwrap();
    }

    #[tokio::test]
    async fn accepted_webrtc_invite_aborts_existing_relay_without_starting_new_task() {
        let config = RemoteConfig::default_for_server("http://127.0.0.1:27880");
        let invite = niuma_core::remote::signaling::ConnectionInvite {
            connection_id: "conn_webrtc".to_string(),
            connection_token: Some("cnt_relay_secret".to_string()),
            client_id: "web_1".to_string(),
            client_label: None,
            transport_preference: niuma_core::remote::signaling::TransportPreference::Webrtc,
            expires_at: "2026-06-28T00:02:00.000Z".to_string(),
        };
        let outbound = vec![serde_json::json!({
            "type": "connection.accept",
            "data": { "connection_id": "conn_webrtc", "transport": "webrtc" }
        })];
        let mut tasks = RelayTaskMap::default();
        let (old_tx, old_rx) = oneshot::channel();
        tasks.insert("conn_old".to_string(), cancellable_relay_task(old_tx));
        tokio::task::yield_now().await;

        apply_connection_invite_relay_lifecycle(
            &mut tasks,
            &config,
            "dvt_secret",
            &invite,
            &outbound,
            |_| panic!("WebRTC accept 不应启动 relay task"),
        );

        assert!(tasks.is_empty());
        old_rx.await.unwrap();
    }

    #[tokio::test]
    async fn accepted_webrtc_invite_keeps_existing_relay_for_same_connection() {
        let config = RemoteConfig::default_for_server("http://127.0.0.1:27880");
        let invite = niuma_core::remote::signaling::ConnectionInvite {
            connection_id: "conn_1".to_string(),
            connection_token: Some("cnt_relay_secret".to_string()),
            client_id: "web_1".to_string(),
            client_label: None,
            transport_preference: niuma_core::remote::signaling::TransportPreference::Webrtc,
            expires_at: "2026-06-28T00:02:00.000Z".to_string(),
        };
        let outbound = vec![serde_json::json!({
            "type": "connection.accept",
            "data": { "connection_id": "conn_1", "transport": "webrtc" }
        })];
        let mut tasks = RelayTaskMap::default();
        let (relay_tx, _relay_rx) = oneshot::channel();
        tasks.insert("conn_1".to_string(), cancellable_relay_task(relay_tx));
        tokio::task::yield_now().await;

        apply_connection_invite_relay_lifecycle(
            &mut tasks,
            &config,
            "dvt_secret",
            &invite,
            &outbound,
            |_| panic!("WebRTC accept 不应启动 relay task"),
        );

        assert!(tasks.contains_key("conn_1"));
    }

    #[tokio::test]
    async fn abort_all_relays_removes_and_aborts_every_task() {
        let mut tasks = RelayTaskMap::default();
        let (first_tx, first_rx) = oneshot::channel();
        let (second_tx, second_rx) = oneshot::channel();
        tasks.insert("conn_1".to_string(), cancellable_relay_task(first_tx));
        tasks.insert("conn_2".to_string(), cancellable_relay_task(second_tx));
        tokio::task::yield_now().await;

        abort_all_relays(&mut tasks);

        assert!(tasks.is_empty());
        first_rx.await.unwrap();
        second_rx.await.unwrap();
    }

    fn cancellable_relay_task(cancelled: oneshot::Sender<()>) -> tokio::task::JoinHandle<()> {
        struct CancelNotice(Option<oneshot::Sender<()>>);

        impl Drop for CancelNotice {
            fn drop(&mut self) {
                if let Some(sender) = self.0.take() {
                    let _ = sender.send(());
                }
            }
        }

        tokio::spawn(async move {
            let _notice = CancelNotice(Some(cancelled));
            std::future::pending::<()>().await;
        })
    }
}

#[cfg(test)]
mod offer_routing_tests {
    use super::*;
    use niuma_core::remote::signaling::{DeviceSignalMessage, SignalOffer};

    #[test]
    fn detects_offer_message_for_async_route() {
        let message = DeviceSignalMessage::SignalOffer {
            id: "msg_1".to_string(),
            data: SignalOffer {
                connection_id: "conn_1".to_string(),
                sdp: "v=0".to_string(),
            },
        };

        assert!(is_async_webrtc_offer(&message));
    }
}
