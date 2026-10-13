//! 评审流仓储 PG 实现(T-204):评论 / 评审 / 提交状态。

use chrono::{DateTime, Utc};
use codeplan_common::error::AppError;
use codeplan_kernel::ids::{RepoId, UserId};
use sqlx::{PgPool, Row};

use crate::domain::PrId;
use crate::protection::{BranchProtection, NewProtection, ProtectionId, ProtectionRepository};
use crate::review::{
    CheckState, CommentId, CommentRepository, CommitStatus, NewComment, NewReview, NewReviewer,
    NewStatus, PrComment, PrReview, PrReviewer, ReviewId, ReviewRepository, ReviewerId,
    ReviewerRepository, Side, StatusId, StatusRepository, Verdict,
};

/// 域错误码前缀。
const DC: &str = "CR";

fn db_err(e: impl std::fmt::Display) -> AppError {
    AppError::internal(DC).with_details(serde_json::json!(e.to_string()))
}

/// 评论 PG 实现。
pub struct PgCommentStore {
    pool: PgPool,
}

impl PgCommentStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn comment_row(row: sqlx::postgres::PgRow) -> PrComment {
    PrComment {
        id: CommentId::from_uuid(row.get::<uuid::Uuid, _>("id")),
        pr_id: PrId::from_uuid(row.get::<uuid::Uuid, _>("pr_id")),
        author: UserId::from_uuid(row.get::<uuid::Uuid, _>("author")),
        body: row.get("body"),
        file_path: row.get("file_path"),
        line: row.get("line"),
        side: row
            .get::<Option<String>, _>("side")
            .as_deref()
            .and_then(Side::from_str_opt),
        anchor_sha: row.get("anchor_sha"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

#[async_trait::async_trait]
impl CommentRepository for PgCommentStore {
    async fn insert(&self, c: &NewComment) -> Result<PrComment, AppError> {
        let (file_path, line, side) = match &c.anchor {
            Some(a) => (
                Some(a.file_path.clone()),
                Some(a.line),
                Some(a.side.as_str().to_owned()),
            ),
            None => (None, None, None),
        };
        let row = sqlx::query(
            "INSERT INTO rev_pr_comment
             (id, pr_id, author, body, file_path, line, side, anchor_sha)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id, pr_id, author, body, file_path, line, side, anchor_sha,
                       created_at, updated_at",
        )
        .bind(CommentId::generate().as_uuid())
        .bind(c.pr_id.as_uuid())
        .bind(c.author.as_uuid())
        .bind(&c.body)
        .bind(&file_path)
        .bind(line)
        .bind(side)
        .bind(&c.anchor_sha)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(comment_row(row))
    }

    async fn list_by_pr(&self, pr_id: PrId) -> Result<Vec<PrComment>, AppError> {
        let rows = sqlx::query(
            "SELECT id, pr_id, author, body, file_path, line, side, anchor_sha,
                    created_at, updated_at
             FROM rev_pr_comment WHERE pr_id = $1 ORDER BY created_at, id",
        )
        .bind(pr_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(comment_row).collect())
    }

    async fn delete(&self, id: CommentId) -> Result<(), AppError> {
        let n = sqlx::query("DELETE FROM rev_pr_comment WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(AppError::not_found(DC, "评论不存在"));
        }
        Ok(())
    }
}

/// 评审 PG 实现。
pub struct PgReviewStore {
    pool: PgPool,
}

impl PgReviewStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn review_row(row: sqlx::postgres::PgRow) -> PrReview {
    PrReview {
        id: ReviewId::from_uuid(row.get::<uuid::Uuid, _>("id")),
        pr_id: PrId::from_uuid(row.get::<uuid::Uuid, _>("pr_id")),
        reviewer: UserId::from_uuid(row.get::<uuid::Uuid, _>("reviewer")),
        verdict: Verdict::from_str_opt(row.get::<&str, _>("verdict")).unwrap_or(Verdict::Comment),
        body: row.get("body"),
        head_sha: row.get("head_sha"),
        created_at: row.get::<DateTime<Utc>, _>("created_at"),
    }
}

#[async_trait::async_trait]
impl ReviewRepository for PgReviewStore {
    async fn insert(&self, r: &NewReview) -> Result<PrReview, AppError> {
        let row = sqlx::query(
            "INSERT INTO rev_pr_review (id, pr_id, reviewer, verdict, body, head_sha)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id, pr_id, reviewer, verdict, body, head_sha, created_at",
        )
        .bind(ReviewId::generate().as_uuid())
        .bind(r.pr_id.as_uuid())
        .bind(r.reviewer.as_uuid())
        .bind(r.verdict.as_str())
        .bind(&r.body)
        .bind(&r.head_sha)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(review_row(row))
    }

    async fn list_by_pr(&self, pr_id: PrId) -> Result<Vec<PrReview>, AppError> {
        let rows = sqlx::query(
            "SELECT id, pr_id, reviewer, verdict, body, head_sha, created_at
             FROM rev_pr_review WHERE pr_id = $1 ORDER BY created_at DESC, id DESC",
        )
        .bind(pr_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(review_row).collect())
    }
}

/// 提交状态 PG 实现。
pub struct PgStatusStore {
    pool: PgPool,
}

impl PgStatusStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn status_row(row: sqlx::postgres::PgRow) -> CommitStatus {
    CommitStatus {
        id: StatusId::from_uuid(row.get::<uuid::Uuid, _>("id")),
        repo_id: RepoId::from_uuid(row.get::<uuid::Uuid, _>("repo_id")),
        commit_sha: row.get("commit_sha"),
        context: row.get("context"),
        state: CheckState::from_str_opt(row.get::<&str, _>("state")).unwrap_or(CheckState::Pending),
        description: row.get("description"),
        target_url: row.get("target_url"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

#[async_trait::async_trait]
impl StatusRepository for PgStatusStore {
    async fn upsert(&self, s: &NewStatus) -> Result<CommitStatus, AppError> {
        let row = sqlx::query(
            "INSERT INTO rev_commit_status
             (id, repo_id, commit_sha, context, state, description, target_url)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (repo_id, commit_sha, context) DO UPDATE
             SET state = $5, description = $6, target_url = $7, updated_at = now()
             RETURNING id, repo_id, commit_sha, context, state, description, target_url,
                       created_at, updated_at",
        )
        .bind(StatusId::generate().as_uuid())
        .bind(s.repo_id.as_uuid())
        .bind(&s.commit_sha)
        .bind(&s.context)
        .bind(s.state.as_str())
        .bind(&s.description)
        .bind(&s.target_url)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(status_row(row))
    }

    async fn list_by_commit(
        &self,
        repo_id: RepoId,
        commit_sha: &str,
    ) -> Result<Vec<CommitStatus>, AppError> {
        let rows = sqlx::query(
            "SELECT id, repo_id, commit_sha, context, state, description, target_url,
                    created_at, updated_at
             FROM rev_commit_status WHERE repo_id = $1 AND commit_sha = $2 ORDER BY context",
        )
        .bind(repo_id.as_uuid())
        .bind(commit_sha)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(status_row).collect())
    }
}

/// 分支保护规则 PG 实现(T-205)。
pub struct PgProtectionStore {
    pool: PgPool,
}

impl PgProtectionStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn protection_row(row: sqlx::postgres::PgRow) -> BranchProtection {
    BranchProtection {
        id: ProtectionId::from_uuid(row.get::<uuid::Uuid, _>("id")),
        repo_id: RepoId::from_uuid(row.get::<uuid::Uuid, _>("repo_id")),
        branch_pattern: row.get("branch_pattern"),
        min_approvals: row.get("min_approvals"),
        required_contexts: row.get("required_contexts"),
        block_force_push: row.get("block_force_push"),
        block_direct_push: row.get("block_direct_push"),
        require_code_owner: row.get("require_code_owner"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

#[async_trait::async_trait]
impl ProtectionRepository for PgProtectionStore {
    async fn upsert(&self, rule: &NewProtection) -> Result<BranchProtection, AppError> {
        let row = sqlx::query(
            "INSERT INTO rev_branch_protection
             (id, repo_id, branch_pattern, min_approvals, required_contexts,
              block_force_push, block_direct_push, require_code_owner)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (repo_id, branch_pattern) DO UPDATE SET
                min_approvals = $4, required_contexts = $5,
                block_force_push = $6, block_direct_push = $7, require_code_owner = $8,
                updated_at = now()
             RETURNING id, repo_id, branch_pattern, min_approvals, required_contexts,
                       block_force_push, block_direct_push, require_code_owner,
                       created_at, updated_at",
        )
        .bind(ProtectionId::generate().as_uuid())
        .bind(rule.repo_id.as_uuid())
        .bind(&rule.branch_pattern)
        .bind(rule.min_approvals)
        .bind(&rule.required_contexts)
        .bind(rule.block_force_push)
        .bind(rule.block_direct_push)
        .bind(rule.require_code_owner)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(protection_row(row))
    }

    async fn list_by_repo(&self, repo_id: RepoId) -> Result<Vec<BranchProtection>, AppError> {
        let rows = sqlx::query(
            "SELECT id, repo_id, branch_pattern, min_approvals, required_contexts,
                    block_force_push, block_direct_push, require_code_owner,
                    created_at, updated_at
             FROM rev_branch_protection WHERE repo_id = $1 ORDER BY branch_pattern",
        )
        .bind(repo_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(protection_row).collect())
    }

    async fn delete(&self, repo_id: RepoId, branch_pattern: &str) -> Result<(), AppError> {
        let n = sqlx::query(
            "DELETE FROM rev_branch_protection WHERE repo_id = $1 AND branch_pattern = $2",
        )
        .bind(repo_id.as_uuid())
        .bind(branch_pattern)
        .execute(&self.pool)
        .await
        .map_err(db_err)?
        .rows_affected();
        if n == 0 {
            return Err(AppError::not_found("CR", "保护规则不存在"));
        }
        Ok(())
    }
}

/// PR 评审人 PG 实现(T-206)。
pub struct PgReviewerStore {
    pool: PgPool,
}

impl PgReviewerStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn reviewer_row(row: sqlx::postgres::PgRow) -> PrReviewer {
    PrReviewer {
        id: ReviewerId::from_uuid(row.get::<uuid::Uuid, _>("id")),
        pr_id: PrId::from_uuid(row.get::<uuid::Uuid, _>("pr_id")),
        reviewer: UserId::from_uuid(row.get::<uuid::Uuid, _>("reviewer")),
        added_by: row
            .try_get::<Option<uuid::Uuid>, _>("added_by")
            .ok()
            .flatten()
            .map(UserId::from_uuid),
        created_at: row.get("created_at"),
    }
}

#[async_trait::async_trait]
impl ReviewerRepository for PgReviewerStore {
    async fn upsert(&self, r: &NewReviewer) -> Result<PrReviewer, AppError> {
        let row = sqlx::query(
            "INSERT INTO rev_pr_reviewer (id, pr_id, reviewer, added_by)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (pr_id, reviewer) DO UPDATE SET added_by = $4
             RETURNING id, pr_id, reviewer, added_by, created_at",
        )
        .bind(ReviewerId::generate().as_uuid())
        .bind(r.pr_id.as_uuid())
        .bind(r.reviewer.as_uuid())
        .bind(r.added_by.map(|u| u.as_uuid()))
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(reviewer_row(row))
    }

    async fn list_by_pr(&self, pr_id: PrId) -> Result<Vec<PrReviewer>, AppError> {
        let rows = sqlx::query(
            "SELECT id, pr_id, reviewer, added_by, created_at
             FROM rev_pr_reviewer WHERE pr_id = $1 ORDER BY created_at, id",
        )
        .bind(pr_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(reviewer_row).collect())
    }

    async fn delete(&self, pr_id: PrId, reviewer: UserId) -> Result<(), AppError> {
        let n = sqlx::query("DELETE FROM rev_pr_reviewer WHERE pr_id = $1 AND reviewer = $2")
            .bind(pr_id.as_uuid())
            .bind(reviewer.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(AppError::not_found("CR", "评审人指派不存在"));
        }
        Ok(())
    }
}
