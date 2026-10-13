//! Release 实体与仓储 trait(T-208):发布 + 附件 + 自动 Release Notes。

use chrono::{DateTime, Utc};
use codeplan_common::{define_id, error::AppError};
use codeplan_kernel::ids::{RepoId, UserId};

define_id!(
    /// Release ID。
    ReleaseId
);
define_id!(
    /// Release 附件 ID。
    ReleaseAssetId
);

/// Release(必须引用仓库中已存在的标签)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Release ID。
    pub id: ReleaseId,
    /// 仓库 ID。
    pub repo_id: RepoId,
    /// 标签名。
    pub tag_name: String,
    /// 标题(空 = 用标签名)。
    pub name: String,
    /// Release Notes(空 = 发布时自动生成)。
    pub body: String,
    /// Notes 是否为自动生成。
    pub notes_auto: bool,
    /// 发布人。
    pub author: UserId,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 新建 Release 入参。
#[derive(Debug, Clone)]
pub struct NewRelease {
    /// 仓库 ID。
    pub repo_id: RepoId,
    /// 标签名。
    pub tag_name: String,
    /// 标题。
    pub name: String,
    /// Notes。
    pub body: String,
    /// Notes 是否自动生成。
    pub notes_auto: bool,
    /// 发布人。
    pub author: UserId,
}

/// Release 附件(内容在对象存储 `release/{release_id}/{filename}`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    /// 附件 ID。
    pub id: ReleaseAssetId,
    /// Release ID。
    pub release_id: ReleaseId,
    /// 文件名。
    pub filename: String,
    /// 字节数。
    pub size: i64,
    /// 上传时间。
    pub created_at: DateTime<Utc>,
}

/// Release 仓储。
#[async_trait::async_trait]
pub trait ReleaseRepository: Send + Sync {
    /// 创建 Release(同仓库同标签冲突 `GIT-409`)。
    ///
    /// # Errors
    /// 存储故障;冲突。
    async fn create(&self, release: &NewRelease) -> Result<Release, AppError>;

    /// 更新标题/Notes(不存在 `GIT-404`)。
    ///
    /// # Errors
    /// 存储故障。
    async fn update(
        &self,
        id: ReleaseId,
        name: Option<&str>,
        body: Option<&str>,
        notes_auto: Option<bool>,
    ) -> Result<Release, AppError>;

    /// 删除 Release(附件记录级联;对象存储内容由服务层清理)。
    ///
    /// # Errors
    /// 存储故障。
    async fn delete(&self, id: ReleaseId) -> Result<(), AppError>;

    /// 仓库 Release 列表(时间倒序)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_repo(&self, repo_id: RepoId) -> Result<Vec<Release>, AppError>;

    /// 按标签取。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_tag(
        &self,
        repo_id: RepoId,
        tag_name: &str,
    ) -> Result<Option<Release>, AppError>;

    /// 追加附件(同 release 同名覆盖)。
    ///
    /// # Errors
    /// 存储故障。
    async fn upsert_asset(
        &self,
        release_id: ReleaseId,
        filename: &str,
        size: i64,
    ) -> Result<ReleaseAsset, AppError>;

    /// 附件列表。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_assets(&self, release_id: ReleaseId) -> Result<Vec<ReleaseAsset>, AppError>;

    /// 删除附件记录(不存在 `GIT-404`)。
    ///
    /// # Errors
    /// 存储故障。
    async fn delete_asset(&self, release_id: ReleaseId, filename: &str) -> Result<(), AppError>;
}
