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

/// SSH 公钥目录:按指纹查用户(SSH 服务认证用,实现见 identity)。
#[async_trait::async_trait]
pub trait SshKeyDirectory: Send + Sync {
    /// 按公钥指纹(SHA256:base64,与 `ssh-keygen -lf` 一致)查用户登录名与 ID。
    ///
    /// # Errors
    /// 目录查询故障。
    async fn user_by_fingerprint(&self, fingerprint: &str) -> Result<Option<SshKeyUser>, String>;
}

/// SSH 认证命中的用户。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshKeyUser {
    /// 用户 ID。
    pub user_id: crate::ids::UserId,
    /// 登录名。
    pub username: String,
}
