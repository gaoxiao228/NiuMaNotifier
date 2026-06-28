use serde_json::{json, Value};
use uuid::Uuid;

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
