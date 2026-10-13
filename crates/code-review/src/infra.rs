//! PR 仓储 PG 实现(SQLx)。

use chrono::{DateTime, Utc};
use codeplan_common::error::AppError;
use codeplan_kernel::ids::{RepoId, UserId};
use sqlx::{PgPool, Row};

use crate::domain::{NewPullRequest, PrEdit, PrId, PrState, PullRequest, PullRequestRepository};

/// PG 实现。
pub struct PgPullRequestStore {
    pool: PgPool,
}

impl PgPullRequestStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_pr(row: sqlx::postgres::PgRow) -> PullRequest {
    PullRequest {
        id: PrId::from_uuid(row.get::<uuid::Uuid, _>("id")),
        repo_id: RepoId::from_uuid(row.get::<uuid::Uuid, _>("repo_id")),
        number: row.get::<i64, _>("number"),
        title: row.get("title"),
        body: row.get("body"),
        source_branch: row.get("source_branch"),
        target_branch: row.get("target_branch"),
        author: UserId::from_uuid(row.get::<uuid::Uuid, _>("author")),
        state: PrState::from_str_opt(row.get::<&str, _>("state")).unwrap_or(PrState::Open),
        is_draft: row.get("is_draft"),
        merge_method: row.get("merge_method"),
        merge_commit: row.get("merge_commit"),
        merged_by: row
            .try_get::<Option<uuid::Uuid>, _>("merged_by")
            .ok()
            .flatten()
            .map(UserId::from_uuid),
        merged_at: row
            .try_get::<Option<DateTime<Utc>>, _>("merged_at")
            .ok()
            .flatten(),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

const COLS: &str = "id, repo_id, number, title, body, source_branch, target_branch, author, \
                    state, is_draft, merge_method, merge_commit, merged_by, merged_at, \
                    created_at, updated_at";

#[async_trait::async_trait]
impl PullRequestRepository for PgPullRequestStore {
    async fn insert(&self, pr: &NewPullRequest) -> Result<PullRequest, AppError> {
        let mut tx =
            self.pool.begin().await.map_err(|e| {
                AppError::internal("CR").with_details(serde_json::json!(e.to_string()))
            })?;
        // 仓库内序号并发安全:advisory 锁串行化同仓库分配
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0))")
            .bind(pr.repo_id.as_uuid().to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::internal("CR").with_details(serde_json::json!(e.to_string())))?;
        let row = sqlx::query(&format!(
            "INSERT INTO rev_pull_request
             (id, repo_id, number, title, body, source_branch, target_branch, author, state, is_draft)
             VALUES ($1, $2,
                     (SELECT COALESCE(MAX(number), 0) + 1 FROM rev_pull_request WHERE repo_id = $2),
                     $3, $4, $5, $6, $7, 'open', $8)
             RETURNING {COLS}"
        ))
        .bind(PrId::generate().as_uuid())
        .bind(pr.repo_id.as_uuid())
        .bind(&pr.title)
        .bind(&pr.body)
        .bind(&pr.source_branch)
        .bind(&pr.target_branch)
        .bind(pr.author.as_uuid())
        .bind(pr.is_draft)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::internal("CR").with_details(serde_json::json!(e.to_string())))?;
        tx.commit()
            .await
            .map_err(|e| AppError::internal("CR").with_details(serde_json::json!(e.to_string())))?;
        Ok(row_to_pr(row))
    }

    async fn find_by_number(
        &self,
        repo_id: RepoId,
        number: i64,
    ) -> Result<Option<PullRequest>, AppError> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM rev_pull_request WHERE repo_id = $1 AND number = $2"
        ))
        .bind(repo_id.as_uuid())
        .bind(number)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::internal("CR").with_details(serde_json::json!(e.to_string())))?;
        Ok(row.map(row_to_pr))
    }

    async fn find_by_id(&self, id: PrId) -> Result<Option<PullRequest>, AppError> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM rev_pull_request WHERE id = $1"
        ))
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::internal("CR").with_details(serde_json::json!(e.to_string())))?;
        Ok(row.map(row_to_pr))
    }

    async fn list_by_repo(
        &self,
        repo_id: RepoId,
        state: Option<PrState>,
    ) -> Result<Vec<PullRequest>, AppError> {
        let rows = if let Some(state) = state {
            sqlx::query(&format!(
                "SELECT {COLS} FROM rev_pull_request WHERE repo_id = $1 AND state = $2 \
                 ORDER BY number DESC"
            ))
            .bind(repo_id.as_uuid())
            .bind(state.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::internal("CR").with_details(serde_json::json!(e.to_string())))?
        } else {
            sqlx::query(&format!(
                "SELECT {COLS} FROM rev_pull_request WHERE repo_id = $1 ORDER BY number DESC"
            ))
            .bind(repo_id.as_uuid())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::internal("CR").with_details(serde_json::json!(e.to_string())))?
        };
        Ok(rows.into_iter().map(row_to_pr).collect())
    }

    async fn update_edit(&self, id: PrId, edit: &PrEdit) -> Result<PullRequest, AppError> {
        let row = sqlx::query(&format!(
            "UPDATE rev_pull_request SET
                title = COALESCE($2, title),
                body = COALESCE($3, body),
                is_draft = COALESCE($4, is_draft),
                updated_at = now()
             WHERE id = $1
             RETURNING {COLS}"
        ))
        .bind(id.as_uuid())
        .bind(edit.title.as_deref())
        .bind(edit.body.as_deref())
        .bind(edit.is_draft)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::internal("CR").with_details(serde_json::json!(e.to_string())))?
        .ok_or_else(|| AppError::not_found("CR", "PR 不存在"))?;
        Ok(row_to_pr(row))
    }

    async fn mark_merged(
        &self,
        id: PrId,
        method: &str,
        merge_commit: &str,
        merged_by: UserId,
    ) -> Result<PullRequest, AppError> {
        let row = sqlx::query(&format!(
            "UPDATE rev_pull_request SET
                state = 'merged', merge_method = $2, merge_commit = $3,
                merged_by = $4, merged_at = now(), updated_at = now(), is_draft = FALSE
             WHERE id = $1
             RETURNING {COLS}"
        ))
        .bind(id.as_uuid())
        .bind(method)
        .bind(merge_commit)
        .bind(merged_by.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::internal("CR").with_details(serde_json::json!(e.to_string())))?
        .ok_or_else(|| AppError::not_found("CR", "PR 不存在"))?;
        Ok(row_to_pr(row))
    }

    async fn set_state(&self, id: PrId, state: PrState) -> Result<PullRequest, AppError> {
        let row = sqlx::query(&format!(
            "UPDATE rev_pull_request SET state = $2, updated_at = now()
             WHERE id = $1 RETURNING {COLS}"
        ))
        .bind(id.as_uuid())
        .bind(state.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::internal("CR").with_details(serde_json::json!(e.to_string())))?
        .ok_or_else(|| AppError::not_found("CR", "PR 不存在"))?;
        Ok(row_to_pr(row))
    }
}
