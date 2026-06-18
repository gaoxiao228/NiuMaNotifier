use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiErrorCode {
    ProtocolParameter,
    MissingRequiredParameter,
    ParameterType,
    ParameterFormat,
    BusinessValidation,
    System,
    Database,
    ServiceUnavailable,
    RouteNotFound,
}

impl ApiErrorCode {
    pub fn code(self) -> i32 {
        match self {
            ApiErrorCode::ProtocolParameter => 100001,
            ApiErrorCode::MissingRequiredParameter => 100002,
            ApiErrorCode::ParameterType => 100003,
            ApiErrorCode::ParameterFormat => 100004,
            ApiErrorCode::BusinessValidation => 100101,
            ApiErrorCode::System => 900001,
            ApiErrorCode::Database => 900002,
            ApiErrorCode::ServiceUnavailable => 900004,
            ApiErrorCode::RouteNotFound => 900005,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub code: i32,
    pub message: String,
    pub data: T,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            code: 0,
            message: "ok".to_string(),
            data,
        }
    }
}

impl ApiResponse<Value> {
    pub fn fail(code: ApiErrorCode, message: impl Into<String>) -> Self {
        // 业务失败也使用统一 envelope；HTTP 状态码由 API 层按协议层/系统层另行决定。
        Self {
            code: code.code(),
            message: message.into(),
            data: Value::Null,
        }
    }
}

#[cfg(test)]
mod tests;
