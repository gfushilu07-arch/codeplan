//! GPG 公钥存储(T-210):上传时以 SHA-256(armor)作为存储标识,
//! 真 OpenPGP 指纹与邮箱归属在验证侧(git-service,sequoia)解析。

use chrono::{DateTime, Utc};
use codeplan_common::{define_id, error::AppError};
use codeplan_kernel::ids::UserId;

define_id!(
    /// GPG 公钥 ID。
    GpgKeyId
);

/// 用户 GPG 公钥。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpgKey {
    /// 公钥 ID。
    pub id: GpgKeyId,
    /// 属主。
    pub user_id: UserId,
    /// 存储标识(SHA-256 of armor,64 hex;非 OpenPGP 指纹)。
    pub fingerprint: String,
    /// ASCII armor 公钥。
    pub public_key: String,
    /// 声明邮箱(展示用)。
    pub email: String,
    /// 上传时间。
    pub created_at: DateTime<Utc>,
}

/// 上传入参。
#[derive(Debug, Clone)]
pub struct NewGpgKey {
    /// 属主。
    pub user_id: UserId,
    /// 存储标识。
    pub fingerprint: String,
    /// armor。
    pub public_key: String,
    /// 声明邮箱。
    pub email: String,
}

/// GPG 公钥仓储。
#[async_trait::async_trait]
pub trait GpgKeyRepository: Send + Sync {
    /// 上传(同 armor 幂等,归属覆盖)。
    ///
    /// # Errors
    /// 存储故障。
    async fn upload(&self, key: &NewGpgKey) -> Result<GpgKey, AppError>;

    /// 用户公钥列表。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_user(&self, user_id: UserId) -> Result<Vec<GpgKey>, AppError>;

    /// 全量(企业内规模小;验证侧按邮箱匹配,T-210)。
    ///
    /// # Errors
    /// 存储故障。
    async fn all(&self) -> Result<Vec<GpgKey>, AppError>;

    /// 删除(不存在 `ORG-404`)。
    ///
    /// # Errors
    /// 存储故障。
    async fn delete(&self, id: GpgKeyId, user_id: UserId) -> Result<(), AppError>;
}
