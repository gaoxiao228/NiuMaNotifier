// Task 6 接入实际 runtime 前，这些配置会先作为 relay runtime 的稳定骨架保留。
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayRuntimeConfig {
    pub socket_url: String,
    pub connection_id: String,
}

impl RelayRuntimeConfig {
    #[allow(dead_code)]
    pub fn new(socket_url: impl Into<String>, connection_id: impl Into<String>) -> Self {
        Self {
            socket_url: socket_url.into(),
            connection_id: connection_id.into(),
        }
    }
}

/// Relay 收发、加密帧 ping/pong 和状态同步由 Task 6 接入。
#[allow(dead_code)]
pub fn relay_runtime_pending() -> bool {
    true
}
