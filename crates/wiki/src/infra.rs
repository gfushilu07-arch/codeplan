//! 仓储的 PostgreSQL 实现(表前缀 `wiki_*`)。

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use codeplan_common::error::AppError;
use codeplan_kernel::ids::{ProjectId, UserId};
use sqlx::PgPool;

use crate::DOMAIN_CODE;
use crate::domain::{Attachment, AttachmentId, PageId, PageRepository, PageSummary, WikiPage};

fn wiki_internal(e: impl std::fmt::Display) -> AppError {
    AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
}

fn unique_conflict(e: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(db) = &e
        && db.constraint().is_some()
    {
        return AppError::conflict(DOMAIN_CODE, "slug 已存在");
    }
    wiki_internal(e)
}

/// 页面仓储 PG 实现。
#[derive(Clone)]
pub struct PgPageRepository {
    pool: PgPool,
}

impl PgPageRepository {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// 行映射。
#[derive(sqlx::FromRow)]
struct PageRow {
    id: uuid::Uuid,
    project_id: uuid::Uuid,
    slug: String,
    title: String,
    content: String,
    parent_id: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<PageRow> for WikiPage {
    type Error = AppError;

    fn try_from(r: PageRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: PageId::from_uuid(r.id),
            project_id: ProjectId::from_uuid(r.project_id),
            slug: r.slug,
            title: r.title,
            content: r.content,
            parent_id: r.parent_id.map(PageId::from_uuid),
            updated_by: r.updated_by.map(UserId::from_uuid),
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

const PAGE_COLS: &str =
    "id, project_id, slug, title, content, parent_id, updated_by, created_at, updated_at";

#[async_trait]
impl PageRepository for PgPageRepository {
    async fn create(
        &self,
        project_id: ProjectId,
        slug: &str,
        title: &str,
        content: &str,
        parent_id: Option<PageId>,
        updated_by: Option<UserId>,
    ) -> Result<WikiPage, AppError> {
        let row = sqlx::query_as::<_, PageRow>(
            "INSERT INTO wiki_page (id, project_id, slug, title, content, parent_id, updated_by) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id, project_id, slug, title, content, \
             parent_id, updated_by, created_at, updated_at",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(project_id.as_uuid())
        .bind(slug)
        .bind(title)
        .bind(content)
        .bind(parent_id.map(|p| p.as_uuid()))
        .bind(updated_by.map(|u| u.as_uuid()))
        .fetch_one(&self.pool)
        .await
        .map_err(unique_conflict)?;
        row.try_into()
    }

    async fn update(
        &self,
        id: PageId,
        title: &str,
        content: &str,
        parent_id: Option<PageId>,
        updated_by: Option<UserId>,
    ) -> Result<WikiPage, AppError> {
        let row = sqlx::query_as::<_, PageRow>(
            "UPDATE wiki_page SET title = $2, content = $3, parent_id = $4, updated_by = $5, \
             updated_at = now() WHERE id = $1 RETURNING id, project_id, slug, title, content, \
             parent_id, updated_by, created_at, updated_at",
        )
        .bind(id.as_uuid())
        .bind(title)
        .bind(content)
        .bind(parent_id.map(|p| p.as_uuid()))
        .bind(updated_by.map(|u| u.as_uuid()))
        .fetch_optional(&self.pool)
        .await
        .map_err(wiki_internal)?
        .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "页面不存在"))?;
        row.try_into()
    }

    async fn find_by_slug(
        &self,
        project_id: ProjectId,
        slug: &str,
    ) -> Result<Option<WikiPage>, AppError> {
        let sql = format!("SELECT {PAGE_COLS} FROM wiki_page WHERE project_id = $1 AND slug = $2");
        let row = sqlx::query_as::<_, PageRow>(&sql)
            .bind(project_id.as_uuid())
            .bind(slug)
            .fetch_optional(&self.pool)
            .await
            .map_err(wiki_internal)?;
        row.map(WikiPage::try_from).transpose()
    }

    async fn find_by_id(&self, id: PageId) -> Result<Option<WikiPage>, AppError> {
        let sql = format!("SELECT {PAGE_COLS} FROM wiki_page WHERE id = $1");
        let row = sqlx::query_as::<_, PageRow>(&sql)
            .bind(id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(wiki_internal)?;
        row.map(WikiPage::try_from).transpose()
    }

    async fn list_by_project(&self, project_id: ProjectId) -> Result<Vec<PageSummary>, AppError> {
        let rows: Vec<(
            uuid::Uuid,
            String,
            String,
            Option<uuid::Uuid>,
            DateTime<Utc>,
        )> = sqlx::query_as(
            "SELECT id, slug, title, parent_id, updated_at FROM wiki_page \
                 WHERE project_id = $1 ORDER BY title",
        )
        .bind(project_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(wiki_internal)?;
        Ok(rows
            .into_iter()
            .map(|(id, slug, title, parent_id, updated_at)| PageSummary {
                id: PageId::from_uuid(id),
                slug,
                title,
                parent_id: parent_id.map(PageId::from_uuid),
                updated_at,
            })
            .collect())
    }

    async fn delete(&self, id: PageId) -> Result<(), AppError> {
        sqlx::query("DELETE FROM wiki_page WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(wiki_internal)?;
        Ok(())
    }
}

/// 附件元数据仓储 PG 实现。
#[derive(Clone)]
pub struct PgAttachmentRepository {
    pool: PgPool,
}

impl PgAttachmentRepository {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl crate::domain::AttachmentRepository for PgAttachmentRepository {
    async fn create(
        &self,
        page_id: PageId,
        filename: &str,
        size_bytes: i64,
        content_type: Option<&str>,
        storage_key: &str,
        uploaded_by: Option<UserId>,
    ) -> Result<Attachment, AppError> {
        let (id, created_at): (uuid::Uuid, DateTime<Utc>) = sqlx::query_as(
            "INSERT INTO wiki_attachment (id, page_id, filename, size_bytes, content_type, \
             storage_key, uploaded_by) VALUES ($1, $2, $3, $4, $5, $6, $7) \
             RETURNING id, created_at",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(page_id.as_uuid())
        .bind(filename)
        .bind(size_bytes)
        .bind(content_type)
        .bind(storage_key)
        .bind(uploaded_by.map(|u| u.as_uuid()))
        .fetch_one(&self.pool)
        .await
        .map_err(wiki_internal)?;
        Ok(Attachment {
            id: AttachmentId::from_uuid(id),
            page_id,
            filename: filename.to_owned(),
            size_bytes,
            content_type: content_type.map(str::to_owned),
            storage_key: storage_key.to_owned(),
            created_at,
        })
    }

    async fn find_by_id(&self, id: AttachmentId) -> Result<Option<Attachment>, AppError> {
        let row: Option<(
            uuid::Uuid,
            uuid::Uuid,
            String,
            i64,
            Option<String>,
            String,
            DateTime<Utc>,
        )> = sqlx::query_as(
            "SELECT id, page_id, filename, size_bytes, content_type, storage_key, created_at \
                 FROM wiki_attachment WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(wiki_internal)?;
        Ok(row.map(
            |(id, page_id, filename, size_bytes, content_type, storage_key, created_at)| {
                Attachment {
                    id: AttachmentId::from_uuid(id),
                    page_id: PageId::from_uuid(page_id),
                    filename,
                    size_bytes,
                    content_type,
                    storage_key,
                    created_at,
                }
            },
        ))
    }

    async fn list_by_page(&self, page_id: PageId) -> Result<Vec<Attachment>, AppError> {
        let rows: Vec<(
            uuid::Uuid,
            String,
            i64,
            Option<String>,
            String,
            DateTime<Utc>,
        )> = sqlx::query_as(
            "SELECT id, filename, size_bytes, content_type, storage_key, created_at \
                 FROM wiki_attachment WHERE page_id = $1 ORDER BY created_at",
        )
        .bind(page_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(wiki_internal)?;
        Ok(rows
            .into_iter()
            .map(
                |(id, filename, size_bytes, content_type, storage_key, created_at)| Attachment {
                    id: AttachmentId::from_uuid(id),
                    page_id,
                    filename,
                    size_bytes,
                    content_type,
                    storage_key,
                    created_at,
                },
            )
            .collect())
    }

    async fn delete(&self, id: AttachmentId) -> Result<Option<String>, AppError> {
        let row: Option<(uuid::Uuid, String)> =
            sqlx::query_as("DELETE FROM wiki_attachment WHERE id = $1 RETURNING id, storage_key")
                .bind(id.as_uuid())
                .fetch_optional(&self.pool)
                .await
                .map_err(wiki_internal)?;
        Ok(row.map(|(_, key)| key))
    }
}
