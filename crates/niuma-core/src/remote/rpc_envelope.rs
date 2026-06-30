use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum RemoteRpcEnvelope {
    #[serde(rename = "request")]
    Request {
        version: u8,
        id: String,
        method: String,
        params: serde_json::Value,
    },
    #[serde(rename = "response")]
    Response {
        version: u8,
        id: String,
        ok: bool,
        result: Option<serde_json::Value>,
        error: Option<RemoteRpcError>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteRpcError {
    pub code: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_request_envelope() {
        let value: RemoteRpcEnvelope = serde_json::from_str(
            r#"{"version":1,"type":"request","id":"req_1","method":"device.get_health","params":{}}"#,
        )
        .unwrap();
        match value {
            RemoteRpcEnvelope::Request { method, .. } => assert_eq!(method, "device.get_health"),
            _ => panic!("expected request"),
        }
    }
}
