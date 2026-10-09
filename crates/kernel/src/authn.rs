//! 令牌认证抽象:HTTP 层(各域 api)经此校验 Bearer 令牌,
//! 实现由 identity 提供、server 装配注入——域间零依赖。

use crate::identity::CurrentUser;

/// 令牌认证器。
#[async_trait::async_trait]
pub trait TokenAuthenticator: Send + Sync {
    /// 校验会话令牌,返回当前用户。
    ///
    /// # Errors
    /// 令牌无效/过期返回错误(由 HTTP 层映射 401)。
    async fn verify(&self, token: &str) -> Result<CurrentUser, String>;
}
