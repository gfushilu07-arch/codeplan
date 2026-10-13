//! 用户上下文:请求经认证后携带的当前用户信息。

use crate::ids::UserId;

/// 当前已认证用户(由认证中间件注入,域代码只读)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentUser {
    /// 用户 ID。
    pub id: UserId,
    /// 显示名。
    pub display_name: String,
    /// 邮箱。
    pub email: String,
    /// 是否平台管理员。
    pub is_platform_admin: bool,
    /// 凭证 scopes(JWT 会话 = `["*"]` 全权;PAT = 细粒度,T-210)。
    pub scopes: Vec<String>,
}

impl CurrentUser {
    /// 是否持有指定 scope(通配 `*` 全权)。
    #[must_use]
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == "*" || s == scope)
    }
}

impl CurrentUser {
    /// 构造当前用户上下文。
    #[must_use]
    pub fn new(id: UserId, display_name: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            id,
            display_name: display_name.into(),
            email: email.into(),
            is_platform_admin: false,
            scopes: vec!["*".to_owned()],
        }
    }

    /// 标记为平台管理员。
    #[must_use]
    pub fn as_platform_admin(mut self) -> Self {
        self.is_platform_admin = true;
        self
    }
}
