//! 仓库元数据仓储的 PostgreSQL 实现(表前缀 `repo_*`)。

use crate::domain::RepoId;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use codeplan_common::error::AppError;
use codeplan_kernel::ids::{ProjectId, UserId};
use sqlx::PgPool;

use crate::DOMAIN_CODE;
use crate::domain::{NewRepository, Repository, RepositoryRepository, Visibility};

fn git_internal(e: impl std::fmt::Display) -> AppError {
    AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
}

fn unique_conflict(e: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(db) = &e
        && db.constraint().is_some()
    {
        return AppError::conflict(DOMAIN_CODE, "仓库路径已存在");
    }
    git_internal(e)
}

/// 仓库元数据仓储 PG 实现。
#[derive(Clone)]
pub struct PgRepositoryStore {
    pool: PgPool,
}

impl PgRepositoryStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// 行 → 实体(自由函数,便于 TryFrom 与仓储共用)。
#[allow(clippy::too_many_arguments)]
fn row_into(
    id: uuid::Uuid,
    full_name: String,
    key: String,
    name: String,
    description: String,
    visibility: String,
    default_branch: String,
    owner: uuid::Uuid,
    project_id: Option<uuid::Uuid>,
    archived: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
) -> Repository {
    Repository {
        id: RepoId::from_uuid(id),
        full_name,
        key,
        name,
        description,
        visibility: Visibility::from_str_opt(&visibility).unwrap_or(Visibility::Private),
        default_branch,
        owner: UserId::from_uuid(owner),
        project_id: project_id.map(ProjectId::from_uuid),
        archived,
        created_at,
        updated_at,
    }
}

/// 行映射。
#[derive(sqlx::FromRow)]
struct RepoRow {
    id: uuid::Uuid,
    full_name: String,
    key: String,
    name: String,
    description: String,
    visibility: String,
    default_branch: String,
    owner: uuid::Uuid,
    project_id: Option<uuid::Uuid>,
    archived: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<RepoRow> for Repository {
    type Error = AppError;

    fn try_from(r: RepoRow) -> Result<Self, Self::Error> {
        Ok(row_into(
            r.id,
            r.full_name,
            r.key,
            r.name,
            r.description,
            r.visibility,
            r.default_branch,
            r.owner,
            r.project_id,
            r.archived,
            r.created_at,
            r.updated_at,
        ))
    }
}

const REPO_COLS: &str = "id, full_name, key, name, description, visibility, default_branch, \
     owner, project_id, archived, created_at, updated_at";

#[async_trait]
impl RepositoryRepository for PgRepositoryStore {
    async fn create(&self, repo: &NewRepository, full_name: &str) -> Result<Repository, AppError> {
        let row = sqlx::query_as::<_, RepoRow>(
            "INSERT INTO repo_repository (id, full_name, key, name, description, visibility, \
             default_branch, owner, project_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
             RETURNING id, full_name, key, name, description, visibility, default_branch, \
             owner, project_id, archived, created_at, updated_at",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(full_name)
        .bind(&repo.key)
        .bind(&repo.name)
        .bind(&repo.description)
        .bind(repo.visibility.as_str())
        .bind(&repo.default_branch)
        .bind(repo.owner.as_uuid())
        .bind(repo.project_id.map(|p| p.as_uuid()))
        .fetch_one(&self.pool)
        .await
        .map_err(unique_conflict)?;
        row.try_into()
    }

    async fn find_by_full_name(&self, full_name: &str) -> Result<Option<Repository>, AppError> {
        let sql = format!("SELECT {REPO_COLS} FROM repo_repository WHERE full_name = $1");
        let row = sqlx::query_as::<_, RepoRow>(&sql)
            .bind(full_name)
            .fetch_optional(&self.pool)
            .await
            .map_err(git_internal)?;
        row.map(Repository::try_from).transpose()
    }

    async fn find_by_id(&self, id: RepoId) -> Result<Option<Repository>, AppError> {
        let sql = format!("SELECT {REPO_COLS} FROM repo_repository WHERE id = $1");
        let row = sqlx::query_as::<_, RepoRow>(&sql)
            .bind(id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(git_internal)?;
        row.map(Repository::try_from).transpose()
    }

    async fn list(&self) -> Result<Vec<Repository>, AppError> {
        let sql = format!("SELECT {REPO_COLS} FROM repo_repository ORDER BY created_at DESC");
        let rows = sqlx::query_as::<_, RepoRow>(&sql)
            .fetch_all(&self.pool)
            .await
            .map_err(git_internal)?;
        rows.into_iter().map(Repository::try_from).collect()
    }

    async fn set_default_branch(&self, id: RepoId, branch: &str) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE repo_repository SET default_branch = $2, updated_at = now() WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(branch)
        .execute(&self.pool)
        .await
        .map_err(git_internal)?;
        Ok(())
    }

    async fn set_archived(&self, id: RepoId, archived: bool) -> Result<(), AppError> {
        sqlx::query("UPDATE repo_repository SET archived = $2, updated_at = now() WHERE id = $1")
            .bind(id.as_uuid())
            .bind(archived)
            .execute(&self.pool)
            .await
            .map_err(git_internal)?;
        Ok(())
    }
}
