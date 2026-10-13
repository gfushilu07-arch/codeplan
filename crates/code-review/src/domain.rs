//! PR 实体与仓储 trait(评审域,纯领域层)。

use chrono::{DateTime, Utc};
use codeplan_common::{define_id, error::AppError};
use codeplan_kernel::ids::{RepoId, UserId};

define_id!(
    /// Pull Request ID。
    PrId
);

/// PR 生命周期状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    /// 开放(可评审/可合并)。
    Open,
    /// 已合并(终态)。
    Merged,
    /// 已关闭(终态,可 reopen 到 open)。
    Closed,
}

impl PrState {
    /// 存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Merged => "merged",
            Self::Closed => "closed",
        }
    }

    /// 从数据库值解析。
    #[must_use]
    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "merged" => Some(Self::Merged),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }
}

/// Pull Request。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    /// PR ID。
    pub id: PrId,
    /// 仓库 ID。
    pub repo_id: RepoId,
    /// 仓库内序号(从 1 递增)。
    pub number: i64,
    /// 标题。
    pub title: String,
    /// 描述。
    pub body: String,
    /// 源分支。
    pub source_branch: String,
    /// 目标分支。
    pub target_branch: String,
    /// 创建人。
    pub author: UserId,
    /// 状态。
    pub state: PrState,
    /// 是否草稿(草稿禁止合并)。
    pub is_draft: bool,
    /// 实际使用的合并方式(合并后回填)。
    pub merge_method: Option<String>,
    /// 合并提交 SHA(合并后回填)。
    pub merge_commit: Option<String>,
    /// 合并操作人。
    pub merged_by: Option<UserId>,
    /// 合并时间。
    pub merged_at: Option<DateTime<Utc>>,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 创建 PR 入参。
#[derive(Debug, Clone)]
pub struct NewPullRequest {
    /// 仓库 ID。
    pub repo_id: RepoId,
    /// 标题。
    pub title: String,
    /// 描述。
    pub body: String,
    /// 源分支。
    pub source_branch: String,
    /// 目标分支。
    pub target_branch: String,
    /// 创建人。
    pub author: UserId,
    /// 是否草稿。
    pub is_draft: bool,
}

/// 编辑 PR 入参(None = 不修改)。
#[derive(Debug, Clone, Default)]
pub struct PrEdit {
    /// 标题。
    pub title: Option<String>,
    /// 描述。
    pub body: Option<String>,
    /// 草稿标记。
    pub is_draft: Option<bool>,
}

/// PR 仓储。
#[async_trait::async_trait]
pub trait PullRequestRepository: Send + Sync {
    /// 插入并分配仓库内序号。
    ///
    /// # Errors
    /// 存储故障。
    async fn insert(&self, pr: &NewPullRequest) -> Result<PullRequest, AppError>;

    /// 按仓库内序号取。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_number(
        &self,
        repo_id: RepoId,
        number: i64,
    ) -> Result<Option<PullRequest>, AppError>;

    /// 按 ID 取。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_id(&self, id: PrId) -> Result<Option<PullRequest>, AppError>;

    /// 按仓库列 PR(可按状态过滤,number 倒序)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_repo(
        &self,
        repo_id: RepoId,
        state: Option<PrState>,
    ) -> Result<Vec<PullRequest>, AppError>;

    /// 编辑标题/描述/草稿标记。
    ///
    /// # Errors
    /// 存储故障;PR 不存在 `CR-404`。
    async fn update_edit(&self, id: PrId, edit: &PrEdit) -> Result<PullRequest, AppError>;

    /// 标记合并(终态)。
    ///
    /// # Errors
    /// 存储故障;PR 不存在 `CR-404`。
    async fn mark_merged(
        &self,
        id: PrId,
        method: &str,
        merge_commit: &str,
        merged_by: UserId,
    ) -> Result<PullRequest, AppError>;

    /// 标记关闭/重开。
    ///
    /// # Errors
    /// 存储故障;PR 不存在 `CR-404`。
    async fn set_state(&self, id: PrId, state: PrState) -> Result<PullRequest, AppError>;
}
