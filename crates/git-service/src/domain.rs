//! Git 仓库实体与仓储 trait(元数据 PG + 裸仓库文件系统)。

use chrono::{DateTime, Utc};
use codeplan_common::define_id;
use codeplan_common::error::AppError;
use codeplan_kernel::ids::ProjectId;
use codeplan_kernel::ids::UserId;

define_id!(
    /// 仓库 ID。
    RepoId
);

/// 仓库可见性(企业内网:private 仅成员,public 全组织可读)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// 私有(仅成员)。
    Private,
    /// 内网公开(任何登录用户可读)。
    Public,
}

impl Visibility {
    /// 存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Public => "public",
        }
    }

    /// 从数据库值解析。
    #[must_use]
    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value {
            "private" => Some(Self::Private),
            "public" => Some(Self::Public),
            _ => None,
        }
    }
}

/// 仓库元数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    /// 仓库 ID。
    pub id: RepoId,
    /// 全名(`{ownerKey}/{key}`,唯一)。
    pub full_name: String,
    /// 仓库键。
    pub key: String,
    /// 名称。
    pub name: String,
    /// 描述。
    pub description: String,
    /// 可见性。
    pub visibility: Visibility,
    /// 默认分支。
    pub default_branch: String,
    /// 所有者。
    pub owner: UserId,
    /// 关联项目(可空)。
    pub project_id: Option<ProjectId>,
    /// 是否已归档。
    pub archived: bool,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 创建仓库入参。
#[derive(Debug, Clone)]
pub struct NewRepository {
    /// 仓库键(小写字母/数字/连字符)。
    pub key: String,
    /// 名称。
    pub name: String,
    /// 描述。
    pub description: String,
    /// 可见性。
    pub visibility: Visibility,
    /// 默认分支。
    pub default_branch: String,
    /// 所有者(创建者)。
    pub owner: UserId,
    /// 关联项目。
    pub project_id: Option<ProjectId>,
}

/// 仓库元数据仓储抽象。
#[async_trait::async_trait]
pub trait RepositoryRepository: Send + Sync {
    /// 创建元数据(full_name 唯一冲突返回 `GIT-409`)。
    ///
    /// # Errors
    /// 存储故障或唯一键冲突。
    async fn create(&self, repo: &NewRepository, full_name: &str) -> Result<Repository, AppError>;

    /// 按全名查询。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_full_name(&self, full_name: &str) -> Result<Option<Repository>, AppError>;

    /// 按 ID 查询。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_id(&self, id: RepoId) -> Result<Option<Repository>, AppError>;

    /// 全量仓库列表(企业内部量级;归档的同样返回,由服务层过滤)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list(&self) -> Result<Vec<Repository>, AppError>;

    /// 更新默认分支。
    ///
    /// # Errors
    /// 存储故障。
    async fn set_default_branch(&self, id: RepoId, branch: &str) -> Result<(), AppError>;

    /// 归档/取消归档。
    ///
    /// # Errors
    /// 存储故障。
    async fn set_archived(&self, id: RepoId, archived: bool) -> Result<(), AppError>;
}

/// 裸仓库文件系统管理抽象(实现见 `crate::storage`)。
#[async_trait::async_trait]
pub trait BareRepoStore: Send + Sync {
    /// `git init --bare`,返回仓库路径(`<root>/{full_name}.git`)。
    ///
    /// # Errors
    /// git 命令失败或键非法。
    async fn init_bare(
        &self,
        full_name: &str,
        default_branch: &str,
    ) -> Result<std::path::PathBuf, String>;

    /// 仓库路径(供 T-121 http-backend 代理与 T-122 gix 读取)。
    #[must_use]
    fn repo_path(&self, full_name: &str) -> std::path::PathBuf;
}
