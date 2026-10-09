//! 统一错误体系:错误码格式 `<域>-<3 位 HTTP 状态>`(如 `GIT-404`)。

use serde::{Deserialize, Serialize};

/// 平台统一错误。域错误枚举经 `From` 转换为本类型后跨 api 边界。
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct AppError {
    code: String,
    message: String,
    http_status: u16,
    details: Option<serde_json::Value>,
}

impl AppError {
    /// 按错误码 + HTTP 状态 + 消息构造。
    #[must_use]
    pub fn new(code: impl Into<String>, http_status: u16, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            http_status,
            details: None,
        }
    }

    /// `<域>-400` 参数校验失败。
    #[must_use]
    pub fn invalid(domain: &str, message: impl Into<String>) -> Self {
        Self::new(format!("{domain}-400"), 400, message)
    }

    /// `<域>-401` 未认证。
    #[must_use]
    pub fn unauthenticated(domain: &str) -> Self {
        Self::new(format!("{domain}-401"), 401, "未认证")
    }

    /// `<域>-403` 无权限。
    #[must_use]
    pub fn forbidden(domain: &str, message: impl Into<String>) -> Self {
        Self::new(format!("{domain}-403"), 403, message)
    }

    /// `<域>-404` 资源不存在。
    #[must_use]
    pub fn not_found(domain: &str, message: impl Into<String>) -> Self {
        Self::new(format!("{domain}-404"), 404, message)
    }

    /// `<域>-409` 冲突(唯一键/状态机非法流转等)。
    #[must_use]
    pub fn conflict(domain: &str, message: impl Into<String>) -> Self {
        Self::new(format!("{domain}-409"), 409, message)
    }

    /// `<域>-500` 内部错误(message 不得包含敏感细节)。
    #[must_use]
    pub fn internal(domain: &str) -> Self {
        Self::new(format!("{domain}-500"), 500, "内部错误")
    }

    /// 错误码(如 `GIT-404`)。
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// 面向用户的消息。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// 对应 HTTP 状态码。
    #[must_use]
    pub fn http_status(&self) -> u16 {
        self.http_status
    }

    /// 附加上下文(仅日志与调试用途)。
    #[must_use]
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    /// 附加上下文。
    #[must_use]
    pub fn details(&self) -> Option<&serde_json::Value> {
        self.details.as_ref()
    }
}
