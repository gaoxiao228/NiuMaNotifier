use chrono::{TimeZone, Utc};
use niuma_core::models::ToolKind;
use niuma_core::tool_session::{ToolSessionListItem, ToolSessionStatus};
use niuma_core::tool_session_rpc::{
    ProviderRpcNotification, ProviderRpcRequest, ProviderRpcResponse, SessionDetailParams,
    SessionSnapshotResult,
};

#[test]
fn tool_session_rpc_request_roundtrips_session_detail_params() {
    let request = ProviderRpcRequest::new(
        "req-1",
        "session_detail",
        SessionDetailParams {
            tool: ToolKind::Codex,
            session_id: "s1".to_string(),
            limit: 20,
            cursor: Some("cursor-1".to_string()),
        },
    )
    .unwrap();

    let json = serde_json::to_string(&request).unwrap();
    let decoded: ProviderRpcRequest = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded.id, "req-1");
    assert_eq!(decoded.method, "session_detail");
    assert_eq!(
        decoded.params_as::<SessionDetailParams>().unwrap(),
        SessionDetailParams {
            tool: ToolKind::Codex,
            session_id: "s1".to_string(),
            limit: 20,
            cursor: Some("cursor-1".to_string()),
        }
    );
}

#[test]
fn tool_session_rpc_response_roundtrips_snapshot_result() {
    let result = SessionSnapshotResult {
        tool: ToolKind::Codex,
        sessions: vec![sample_session("s1")],
    };
    let response = ProviderRpcResponse::success("req-1", &result).unwrap();

    let json = serde_json::to_string(&response).unwrap();
    let decoded: ProviderRpcResponse = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded.id, "req-1");
    assert!(decoded.error.is_none());
    assert_eq!(
        decoded.result_as::<SessionSnapshotResult>().unwrap(),
        result
    );
}

#[test]
fn tool_session_rpc_notification_roundtrips_snapshot_update() {
    let result = SessionSnapshotResult {
        tool: ToolKind::Codex,
        sessions: vec![sample_session("s2")],
    };
    let notification = ProviderRpcNotification::new("session_snapshot_updated", &result).unwrap();

    let json = serde_json::to_string(&notification).unwrap();
    let decoded: ProviderRpcNotification = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded.method, "session_snapshot_updated");
    assert_eq!(
        decoded.params_as::<SessionSnapshotResult>().unwrap(),
        result
    );
}

fn sample_session(session_id: &str) -> ToolSessionListItem {
    ToolSessionListItem {
        id: format!("codex:{session_id}"),
        tool: ToolKind::Codex,
        session_id: session_id.to_string(),
        project_path: "/tmp/demo".to_string(),
        project_name: "demo".to_string(),
        file_path: format!("/tmp/demo/{session_id}.jsonl"),
        modified_at: Utc.timestamp_opt(20, 0).single().unwrap(),
        discovered_at: Utc.timestamp_opt(1, 0).single().unwrap(),
        last_seen_at: Utc.timestamp_opt(30, 0).single().unwrap(),
        is_active: true,
        is_subagent: false,
        parent_session_id: None,
        status: ToolSessionStatus::Active,
    }
}
