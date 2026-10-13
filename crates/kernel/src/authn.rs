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

/// 用户 GPG 公钥条目(提交签名验证用,T-210)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpgKeyEntry {
    /// 属主用户。
    pub user_id: crate::ids::UserId,
    /// 存储标识(armor 的 SHA-256)。
    pub key_id: String,
    /// ASCII armor 公钥。
    pub armor: String,
}

/// SSH 公钥条目(全量,签名验证构建 allowed_signers 用)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshPublicKeyEntry {
    /// 公钥指纹(SHA256:base64)。
    pub fingerprint: String,
    /// 公钥文本。
    pub public_key: String,
}

/// GPG 公钥目录(实现:identity;验证:git-service/sequoia)。
#[async_trait::async_trait]
pub trait GpgKeyDirectory: Send + Sync {
    /// 全量公钥(企业内规模小;验证侧按提交者邮箱匹配)。
    ///
    /// # Errors
    /// 目录查询故障。
    async fn all_keys(&self) -> Result<Vec<GpgKeyEntry>, String>;
}

/// SSH 公钥目录扩展:全量公钥(企业内规模小;提交 SSH 签名验证用,T-210)。
#[async_trait::async_trait]
pub trait SshKeyDirectoryAll: Send + Sync {
    /// 全量公钥。
    ///
    /// # Errors
    /// 目录查询故障。
    async fn all_keys(&self) -> Result<Vec<SshPublicKeyEntry>, String>;
}
