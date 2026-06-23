use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

use crate::models::ToolKind;
use crate::tool_session::{ToolSessionDetail, ToolSessionListItem};

// provider RPC 使用 JSON Lines 传输；每一行都是一个请求、响应或通知对象。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderRpcRequest {
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl ProviderRpcRequest {
    pub fn new(
        id: impl Into<String>,
        method: impl Into<String>,
        params: impl Serialize,
    ) -> Result<Self, String> {
        Ok(Self {
            id: id.into(),
            method: method.into(),
            params: serde_json::to_value(params)
                .map_err(|error| format!("序列化 provider 请求参数失败：{error}"))?,
        })
    }

    pub fn params_as<T: DeserializeOwned>(&self) -> Result<T, String> {
        serde_json::from_value(self.params.clone())
            .map_err(|error| format!("解析 provider 请求参数失败：{error}"))
    }
}

// response 的 result/error 二选一；宿主按 id 匹配挂起请求。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderRpcResponse {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ProviderRpcError>,
}

impl ProviderRpcResponse {
    pub fn success(id: impl Into<String>, result: impl Serialize) -> Result<Self, String> {
        Ok(Self {
            id: id.into(),
            result: Some(
                serde_json::to_value(result)
                    .map_err(|error| format!("序列化 provider 响应结果失败：{error}"))?,
            ),
            error: None,
        })
    }

    pub fn failure(
        id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            result: None,
            error: Some(ProviderRpcError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }

    pub fn result_as<T: DeserializeOwned>(&self) -> Result<T, String> {
        let Some(result) = &self.result else {
            let message = self
                .error
                .as_ref()
                .map(|error| error.message.clone())
                .unwrap_or_else(|| "provider 响应缺少 result".to_string());
            return Err(message);
        };
        serde_json::from_value(result.clone())
            .map_err(|error| format!("解析 provider 响应结果失败：{error}"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderRpcError {
    pub code: String,
    pub message: String,
}

// notification 没有 id，不要求响应；当前只消费 session_snapshot_updated。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderRpcNotification {
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl ProviderRpcNotification {
    pub fn new(method: impl Into<String>, params: impl Serialize) -> Result<Self, String> {
        Ok(Self {
            method: method.into(),
            params: serde_json::to_value(params)
                .map_err(|error| format!("序列化 provider 通知参数失败：{error}"))?,
        })
    }

    pub fn params_as<T: DeserializeOwned>(&self) -> Result<T, String> {
        serde_json::from_value(self.params.clone())
            .map_err(|error| format!("解析 provider 通知参数失败：{error}"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionSnapshotParams {
    pub tool: ToolKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionSnapshotResult {
    pub tool: ToolKind,
    pub sessions: Vec<ToolSessionListItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionDetailParams {
    pub tool: ToolKind,
    pub session_id: String,
    // 调用方在发送 RPC 前必须完成缺省值和上限归一化，provider 只接收确定的分页数量。
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionDetailResult {
    pub detail: ToolSessionDetail,
}
