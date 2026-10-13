//! 评审流实体与仓储 trait(T-204):行内评论、评审(通过/驳回)、提交状态检查。

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use codeplan_common::{define_id, error::AppError};
use codeplan_kernel::ids::{RepoId, UserId};

define_id!(
    /// PR 评论 ID。
    CommentId
);
define_id!(
    /// 评审记录 ID。
    ReviewId
);
define_id!(
    /// 提交状态 ID。
    StatusId
);
define_id!(
    /// PR 评审人指派 ID。
    ReviewerId
);

/// diff 侧(行内评论锚定)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// 旧版本(删除行)。
    Left,
    /// 新版本(新增行)。
    Right,
}

impl Side {
    /// 存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
        }
    }

    /// 从数据库值解析。
    #[must_use]
    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value {
            "LEFT" => Some(Self::Left),
            "RIGHT" => Some(Self::Right),
            _ => None,
        }
    }
}

/// PR 评论(无锚定 = 对话流评论;有锚定 = 行内评论)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrComment {
    /// 评论 ID。
    pub id: CommentId,
    /// PR ID。
    pub pr_id: crate::domain::PrId,
    /// 作者。
    pub author: UserId,
    /// 内容。
    pub body: String,
    /// 锚定文件(None = 对话流)。
    pub file_path: Option<String>,
    /// 锚定行号。
    pub line: Option<i32>,
    /// 锚定侧。
    pub side: Option<Side>,
    /// 锚定时的源分支头(用于过期判定)。
    pub anchor_sha: Option<String>,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 行内锚定入参。
#[derive(Debug, Clone)]
pub struct InlineAnchor {
    /// 文件路径(必须出现在当前 diff 中)。
    pub file_path: String,
    /// 行号(>0)。
    pub line: i32,
    /// 侧。
    pub side: Side,
}

/// 新评论入参。
#[derive(Debug, Clone)]
pub struct NewComment {
    /// PR ID。
    pub pr_id: crate::domain::PrId,
    /// 作者。
    pub author: UserId,
    /// 内容。
    pub body: String,
    /// 行内锚定(None = 对话流)。
    pub anchor: Option<InlineAnchor>,
    /// 锚定时源分支头。
    pub anchor_sha: Option<String>,
}

/// 评审结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 通过。
    Approved,
    /// 驳回(要求修改)。
    ChangesRequested,
    /// 仅评论。
    Comment,
}

impl Verdict {
    /// 存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::ChangesRequested => "changes_requested",
            Self::Comment => "comment",
        }
    }

    /// 从数据库值解析。
    #[must_use]
    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value {
            "approved" => Some(Self::Approved),
            "changes_requested" => Some(Self::ChangesRequested),
            "comment" => Some(Self::Comment),
            _ => None,
        }
    }
}

/// 评审记录(历史保留;聚合时取每人最新一条)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrReview {
    /// 评审 ID。
    pub id: ReviewId,
    /// PR ID。
    pub pr_id: crate::domain::PrId,
    /// 评审人。
    pub reviewer: UserId,
    /// 结论。
    pub verdict: Verdict,
    /// 说明。
    pub body: String,
    /// 评审时源分支头。
    pub head_sha: Option<String>,
    /// 时间。
    pub created_at: DateTime<Utc>,
}

/// 新评审入参。
#[derive(Debug, Clone)]
pub struct NewReview {
    /// PR ID。
    pub pr_id: crate::domain::PrId,
    /// 评审人。
    pub reviewer: UserId,
    /// 结论。
    pub verdict: Verdict,
    /// 说明。
    pub body: String,
    /// 源分支头。
    pub head_sha: Option<String>,
}

/// 检查状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    /// 成功。
    Success,
    /// 失败。
    Failure,
    /// 进行中。
    Pending,
    /// 错误(执行异常)。
    Error,
}

impl CheckState {
    /// 存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Pending => "pending",
            Self::Error => "error",
        }
    }

    /// 从数据库值解析。
    #[must_use]
    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value {
            "success" => Some(Self::Success),
            "failure" => Some(Self::Failure),
            "pending" => Some(Self::Pending),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// 提交状态检查(同 repo+sha+context 覆盖更新)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitStatus {
    /// 状态 ID。
    pub id: StatusId,
    /// 仓库 ID。
    pub repo_id: RepoId,
    /// 提交 SHA。
    pub commit_sha: String,
    /// 检查上下文(如 `ci/build`)。
    pub context: String,
    /// 状态。
    pub state: CheckState,
    /// 描述。
    pub description: String,
    /// 详情链接。
    pub target_url: Option<String>,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 新状态入参。
#[derive(Debug, Clone)]
pub struct NewStatus {
    /// 仓库 ID。
    pub repo_id: RepoId,
    /// 提交 SHA。
    pub commit_sha: String,
    /// 上下文。
    pub context: String,
    /// 状态。
    pub state: CheckState,
    /// 描述。
    pub description: String,
    /// 详情链接。
    pub target_url: Option<String>,
}

/// 聚合结果:无检查 = none;任一 failure/error = failure;任一 pending = pending;否则 success。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusSummary {
    /// 聚合状态。
    pub combined: String,
    /// 各检查明细。
    pub checks: Vec<CommitStatus>,
}

impl StatusSummary {
    /// 由明细聚合。
    #[must_use]
    pub fn aggregate(mut checks: Vec<CommitStatus>) -> Self {
        checks.sort_by(|a, b| a.context.cmp(&b.context));
        let combined = if checks.is_empty() {
            "none"
        } else if checks
            .iter()
            .any(|c| matches!(c.state, CheckState::Failure | CheckState::Error))
        {
            "failure"
        } else if checks.iter().any(|c| c.state == CheckState::Pending) {
            "pending"
        } else {
            "success"
        };
        Self {
            combined: combined.to_owned(),
            checks,
        }
    }
}

/// 评论仓储。
#[async_trait::async_trait]
pub trait CommentRepository: Send + Sync {
    /// 插入评论。
    ///
    /// # Errors
    /// 存储故障。
    async fn insert(&self, comment: &NewComment) -> Result<PrComment, AppError>;

    /// PR 全部评论(时间正序)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_pr(&self, pr_id: crate::domain::PrId) -> Result<Vec<PrComment>, AppError>;

    /// 删除评论(不存在 `CR-404`)。
    ///
    /// # Errors
    /// 存储故障。
    async fn delete(&self, id: CommentId) -> Result<(), AppError>;
}

/// 评审仓储。
#[async_trait::async_trait]
pub trait ReviewRepository: Send + Sync {
    /// 追加评审记录。
    ///
    /// # Errors
    /// 存储故障。
    async fn insert(&self, review: &NewReview) -> Result<PrReview, AppError>;

    /// PR 全部评审(时间倒序)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_pr(&self, pr_id: crate::domain::PrId) -> Result<Vec<PrReview>, AppError>;
}

/// 提交状态仓储。
#[async_trait::async_trait]
pub trait StatusRepository: Send + Sync {
    /// 覆盖写入(upsert)。
    ///
    /// # Errors
    /// 存储故障。
    async fn upsert(&self, status: &NewStatus) -> Result<CommitStatus, AppError>;

    /// 提交全部检查(按 context 排序)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_commit(
        &self,
        repo_id: RepoId,
        commit_sha: &str,
    ) -> Result<Vec<CommitStatus>, AppError>;
}

/// 评审聚合结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewSummary {
    /// 当前通过人(按用户 ID,稳定排序)。
    pub approvals: Vec<String>,
    /// 当前驳回人。
    pub changes_requested: Vec<String>,
    /// 聚合结论:approved / changes_requested / pending。
    pub combined: String,
}

/// 评审聚合:每人取最新一条(输入按时间倒序)。
#[must_use]
pub fn aggregate_reviews(reviews: &[PrReview]) -> ReviewSummary {
    let mut latest: HashMap<UserId, Verdict> = HashMap::new();
    for r in reviews {
        latest.entry(r.reviewer).or_insert(r.verdict);
    }
    let mut approvals: Vec<String> = latest
        .iter()
        .filter(|(_, v)| **v == Verdict::Approved)
        .map(|(u, _)| u.as_uuid().to_string())
        .collect();
    let mut changes_requested: Vec<String> = latest
        .iter()
        .filter(|(_, v)| **v == Verdict::ChangesRequested)
        .map(|(u, _)| u.as_uuid().to_string())
        .collect();
    approvals.sort();
    changes_requested.sort();
    let combined = if !changes_requested.is_empty() {
        "changes_requested"
    } else if !approvals.is_empty() {
        "approved"
    } else {
        "pending"
    };
    ReviewSummary {
        approvals,
        changes_requested,
        combined: combined.to_owned(),
    }
}

/// PR 评审人指派(CODEOWNERS 自动指派或人工添加)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrReviewer {
    /// 指派 ID。
    pub id: ReviewerId,
    /// PR ID。
    pub pr_id: crate::domain::PrId,
    /// 被指派人。
    pub reviewer: UserId,
    /// 添加人(None = 系统按 CODEOWNERS 自动指派)。
    pub added_by: Option<UserId>,
    /// 时间。
    pub created_at: DateTime<Utc>,
}

/// 新评审人入参。
#[derive(Debug, Clone)]
pub struct NewReviewer {
    /// PR ID。
    pub pr_id: crate::domain::PrId,
    /// 被指派人。
    pub reviewer: UserId,
    /// 添加人。
    pub added_by: Option<UserId>,
}

/// 评审人仓储。
#[async_trait::async_trait]
pub trait ReviewerRepository: Send + Sync {
    /// 指派(重复幂等)。
    ///
    /// # Errors
    /// 存储故障。
    async fn upsert(&self, reviewer: &NewReviewer) -> Result<PrReviewer, AppError>;

    /// PR 全部被指派评审人(时间正序)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_pr(&self, pr_id: crate::domain::PrId) -> Result<Vec<PrReviewer>, AppError>;

    /// 移除指派(不存在 `CR-404`)。
    ///
    /// # Errors
    /// 存储故障。
    async fn delete(&self, pr_id: crate::domain::PrId, reviewer: UserId) -> Result<(), AppError>;
}
