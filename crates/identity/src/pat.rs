//! 个人访问令牌(T-210):签发(明文仅返回一次)、校验(哈希比对)、吊销。

use chrono::{DateTime, Utc};
use codeplan_common::{define_id, error::AppError};
use codeplan_kernel::ids::UserId;
use sha2::{Digest, Sha256};

define_id!(
    /// PAT ID。
    PatId
);

/// PAT 前缀(明文形态 `cpat_<32hex>`;存储仅存 SHA-256)。
pub const TOKEN_PREFIX: &str = "cpat_";

/// 可授予的 scope。
pub const SCOPES: [&str; 3] = ["api", "repo:read", "repo:write"];

/// 个人访问令牌(存储形态)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonalAccessToken {
    /// PAT ID。
    pub id: PatId,
    /// 属主。
    pub user_id: UserId,
    /// 名称。
    pub name: String,
    /// 令牌 SHA-256(小写 hex)。
    pub token_hash: String,
    /// 明文前缀(展示用,如 `cpat_ab12`)。
    pub prefix: String,
    /// scopes。
    pub scopes: Vec<String>,
    /// 过期时间(空 = 永不过期)。
    pub expires_at: Option<DateTime<Utc>>,
    /// 最近使用时间。
    pub last_used_at: Option<DateTime<Utc>>,
    /// 吊销时间。
    pub revoked_at: Option<DateTime<Utc>>,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 签发入参。
#[derive(Debug, Clone)]
pub struct NewPat {
    /// 属主。
    pub user_id: UserId,
    /// 名称。
    pub name: String,
    /// 令牌哈希。
    pub token_hash: String,
    /// 明文前缀。
    pub prefix: String,
    /// scopes。
    pub scopes: Vec<String>,
    /// 过期时间。
    pub expires_at: Option<DateTime<Utc>>,
}

/// PAT 仓储。
#[async_trait::async_trait]
pub trait PatStore: Send + Sync {
    /// 签发。
    ///
    /// # Errors
    /// 存储故障。
    async fn create(&self, pat: &NewPat) -> Result<PersonalAccessToken, AppError>;

    /// 按哈希取(认证路径)。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_hash(&self, token_hash: &str)
    -> Result<Option<PersonalAccessToken>, AppError>;

    /// 用户令牌列表。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_user(&self, user_id: UserId) -> Result<Vec<PersonalAccessToken>, AppError>;

    /// 吊销(不存在 `ORG-404`)。
    ///
    /// # Errors
    /// 存储故障。
    async fn revoke(&self, id: PatId, user_id: UserId) -> Result<(), AppError>;

    /// 标记使用时间(尽力而为)。
    async fn touch(&self, id: PatId);
}

/// 生成新令牌(明文)。
#[must_use]
pub fn generate_token() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 16];
    rand::rng().fill_bytes(&mut buf);
    let hex: String = buf.iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
        s
    });
    format!("{TOKEN_PREFIX}{hex}")
}

/// 令牌 → SHA-256 hex。
#[must_use]
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// 校验 scope 列表合法。
#[must_use]
pub fn scopes_valid(scopes: &[String]) -> bool {
    !scopes.is_empty() && scopes.iter().all(|s| SCOPES.contains(&s.as_str()))
}

/// 认证判定:令牌有效返回 (user_id, scopes)。
#[must_use]
pub fn accept(pat: &PersonalAccessToken, now: DateTime<Utc>) -> bool {
    pat.revoked_at.is_none() && pat.expires_at.is_none_or(|e| e > now)
}
