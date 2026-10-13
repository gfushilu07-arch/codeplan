//! Release 仓储 PG 实现(T-208)。

use chrono::{DateTime, Utc};
use codeplan_common::error::AppError;
use codeplan_kernel::ids::{RepoId, UserId};
use sqlx::{PgPool, Row};

use crate::release::{
    NewRelease, Release, ReleaseAsset, ReleaseAssetId, ReleaseId, ReleaseRepository,
};

const DC: &str = "GIT";

fn db_err(e: impl std::fmt::Display) -> AppError {
    AppError::internal(DC).with_details(serde_json::json!(e.to_string()))
}

/// PG 实现。
pub struct PgReleaseStore {
    pool: PgPool,
}

impl PgReleaseStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const REL_COLS: &str =
    "id, repo_id, tag_name, name, body, notes_auto, author, created_at, updated_at";

fn release_row(row: sqlx::postgres::PgRow) -> Release {
    Release {
        id: ReleaseId::from_uuid(row.get::<uuid::Uuid, _>("id")),
        repo_id: RepoId::from_uuid(row.get::<uuid::Uuid, _>("repo_id")),
        tag_name: row.get("tag_name"),
        name: row.get("name"),
        body: row.get("body"),
        notes_auto: row.get("notes_auto"),
        author: UserId::from_uuid(row.get::<uuid::Uuid, _>("author")),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

#[async_trait::async_trait]
impl ReleaseRepository for PgReleaseStore {
    async fn create(&self, r: &NewRelease) -> Result<Release, AppError> {
        let row = sqlx::query(&format!(
            "INSERT INTO repo_release (id, repo_id, tag_name, name, body, notes_auto, author)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (repo_id, tag_name) DO UPDATE SET
                name = $4, body = $5, notes_auto = $6, updated_at = now()
             RETURNING {REL_COLS}"
        ))
        .bind(ReleaseId::generate().as_uuid())
        .bind(r.repo_id.as_uuid())
        .bind(&r.tag_name)
        .bind(&r.name)
        .bind(&r.body)
        .bind(r.notes_auto)
        .bind(r.author.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(release_row(row))
    }

    async fn update(
        &self,
        id: ReleaseId,
        name: Option<&str>,
        body: Option<&str>,
        notes_auto: Option<bool>,
    ) -> Result<Release, AppError> {
        let row = sqlx::query(&format!(
            "UPDATE repo_release SET
                name = COALESCE($2, name),
                body = COALESCE($3, body),
                notes_auto = COALESCE($4, notes_auto),
                updated_at = now()
             WHERE id = $1
             RETURNING {REL_COLS}"
        ))
        .bind(id.as_uuid())
        .bind(name)
        .bind(body)
        .bind(notes_auto)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AppError::not_found(DC, "Release 不存在"))?;
        Ok(release_row(row))
    }

    async fn delete(&self, id: ReleaseId) -> Result<(), AppError> {
        let n = sqlx::query("DELETE FROM repo_release WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(AppError::not_found(DC, "Release 不存在"));
        }
        Ok(())
    }

    async fn list_by_repo(&self, repo_id: RepoId) -> Result<Vec<Release>, AppError> {
        let rows = sqlx::query(&format!(
            "SELECT {REL_COLS} FROM repo_release WHERE repo_id = $1 ORDER BY created_at DESC"
        ))
        .bind(repo_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(release_row).collect())
    }

    async fn find_by_tag(
        &self,
        repo_id: RepoId,
        tag_name: &str,
    ) -> Result<Option<Release>, AppError> {
        let row = sqlx::query(&format!(
            "SELECT {REL_COLS} FROM repo_release WHERE repo_id = $1 AND tag_name = $2"
        ))
        .bind(repo_id.as_uuid())
        .bind(tag_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(release_row))
    }

    async fn upsert_asset(
        &self,
        release_id: ReleaseId,
        filename: &str,
        size: i64,
    ) -> Result<ReleaseAsset, AppError> {
        let row = sqlx::query(
            "INSERT INTO repo_release_asset (id, release_id, filename, size)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (release_id, filename) DO UPDATE SET size = $4
             RETURNING id, release_id, filename, size, created_at",
        )
        .bind(ReleaseAssetId::generate().as_uuid())
        .bind(release_id.as_uuid())
        .bind(filename)
        .bind(size)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(ReleaseAsset {
            id: ReleaseAssetId::from_uuid(row.get::<uuid::Uuid, _>("id")),
            release_id: ReleaseId::from_uuid(row.get::<uuid::Uuid, _>("release_id")),
            filename: row.get("filename"),
            size: row.get("size"),
            created_at: row.get::<DateTime<Utc>, _>("created_at"),
        })
    }

    async fn list_assets(&self, release_id: ReleaseId) -> Result<Vec<ReleaseAsset>, AppError> {
        let rows = sqlx::query(
            "SELECT id, release_id, filename, size, created_at
             FROM repo_release_asset WHERE release_id = $1 ORDER BY filename",
        )
        .bind(release_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|row| ReleaseAsset {
                id: ReleaseAssetId::from_uuid(row.get::<uuid::Uuid, _>("id")),
                release_id: ReleaseId::from_uuid(row.get::<uuid::Uuid, _>("release_id")),
                filename: row.get("filename"),
                size: row.get("size"),
                created_at: row.get("created_at"),
            })
            .collect())
    }

    async fn delete_asset(&self, release_id: ReleaseId, filename: &str) -> Result<(), AppError> {
        let n =
            sqlx::query("DELETE FROM repo_release_asset WHERE release_id = $1 AND filename = $2")
                .bind(release_id.as_uuid())
                .bind(filename)
                .execute(&self.pool)
                .await
                .map_err(db_err)?
                .rows_affected();
        if n == 0 {
            return Err(AppError::not_found(DC, "附件不存在"));
        }
        Ok(())
    }
}
