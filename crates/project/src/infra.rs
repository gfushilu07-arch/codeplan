//! 仓储的 PostgreSQL 实现(表前缀 `proj_*`)。

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use codeplan_common::error::AppError;
use codeplan_kernel::ids::UserId;
use sqlx::PgPool;

use crate::DOMAIN_CODE;
use crate::domain::{
    ItemType, NewProject, NewWorkItem, Priority, Project, ProjectId, ProjectRepository, WorkItem,
    WorkItemRepository,
};

fn pm_internal(e: impl std::fmt::Display) -> AppError {
    AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
}

fn unique_conflict(e: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(db) = &e
        && db.constraint().is_some()
    {
        return AppError::conflict(DOMAIN_CODE, "标识或编号已存在");
    }
    pm_internal(e)
}

/// 项目仓储 PG 实现。
#[derive(Clone)]
pub struct PgProjectRepository {
    pool: PgPool,
}

impl PgProjectRepository {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ProjectRepository for PgProjectRepository {
    async fn create(&self, project: &NewProject) -> Result<Project, AppError> {
        let (id, created_at): (uuid::Uuid, DateTime<Utc>) = sqlx::query_as(
            "INSERT INTO proj_project (id, key, name, description, lead) \
             VALUES ($1, $2, $3, $4, $5) RETURNING id, created_at",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(&project.key)
        .bind(&project.name)
        .bind(&project.description)
        .bind(project.lead.map(|u| u.as_uuid()))
        .fetch_one(&self.pool)
        .await
        .map_err(unique_conflict)?;
        Ok(Project {
            id: ProjectId::from_uuid(id),
            key: project.key.clone(),
            name: project.name.clone(),
            description: project.description.clone(),
            lead: project.lead,
            created_at,
        })
    }

    async fn find_by_key(&self, key: &str) -> Result<Option<Project>, AppError> {
        let row: Option<(
            uuid::Uuid,
            String,
            String,
            String,
            Option<uuid::Uuid>,
            DateTime<Utc>,
        )> = sqlx::query_as(
            "SELECT id, key, name, description, lead, created_at \
                 FROM proj_project WHERE key = $1",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(pm_internal)?;
        Ok(
            row.map(|(id, key, name, description, lead, created_at)| Project {
                id: ProjectId::from_uuid(id),
                key,
                name,
                description,
                lead: lead.map(UserId::from_uuid),
                created_at,
            }),
        )
    }

    async fn list(&self) -> Result<Vec<Project>, AppError> {
        let rows: Vec<(
            uuid::Uuid,
            String,
            String,
            String,
            Option<uuid::Uuid>,
            DateTime<Utc>,
        )> = sqlx::query_as(
            "SELECT id, key, name, description, lead, created_at \
                 FROM proj_project ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(pm_internal)?;
        Ok(rows
            .into_iter()
            .map(|(id, key, name, description, lead, created_at)| Project {
                id: ProjectId::from_uuid(id),
                key,
                name,
                description,
                lead: lead.map(UserId::from_uuid),
                created_at,
            })
            .collect())
    }
}

/// 工作项仓储 PG 实现。
#[derive(Clone)]
pub struct PgWorkItemRepository {
    pool: PgPool,
}

impl PgWorkItemRepository {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// 行映射(内部)。
#[derive(sqlx::FromRow)]
struct ItemRow {
    id: uuid::Uuid,
    project_id: uuid::Uuid,
    item_number: i64,
    item_type: String,
    title: String,
    description: String,
    status: String,
    priority: String,
    assignee: Option<uuid::Uuid>,
    reporter: Option<uuid::Uuid>,
    custom_fields: serde_json::Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ItemRow> for WorkItem {
    type Error = AppError;

    fn try_from(r: ItemRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: crate::domain::WorkItemId::from_uuid(r.id),
            project_id: ProjectId::from_uuid(r.project_id),
            item_number: r.item_number,
            item_type: ItemType::from_str_opt(&r.item_type)
                .ok_or_else(|| AppError::internal(DOMAIN_CODE))?,
            title: r.title,
            description: r.description,
            status: r.status,
            priority: Priority::from_str_opt(&r.priority).unwrap_or(Priority::Medium),
            assignee: r.assignee.map(UserId::from_uuid),
            reporter: r.reporter.map(UserId::from_uuid),
            custom_fields: r.custom_fields,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

const ITEM_COLS: &str = "id, project_id, item_number, item_type, title, description, status, \
     priority, assignee, reporter, custom_fields, created_at, updated_at";

#[async_trait]
impl WorkItemRepository for PgWorkItemRepository {
    async fn create(
        &self,
        project_id: ProjectId,
        item: &NewWorkItem,
        initial_status: &str,
        reporter: Option<UserId>,
    ) -> Result<WorkItem, AppError> {
        // 项目内编号:advisory 锁 + max+1,避免并发编号冲突
        let mut tx = self.pool.begin().await.map_err(pm_internal)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(project_id.as_uuid().to_string())
            .execute(&mut *tx)
            .await
            .map_err(pm_internal)?;
        let (number,): (i64,) = sqlx::query_as(
            "SELECT COALESCE(MAX(item_number), 0) + 1 FROM proj_work_item WHERE project_id = $1",
        )
        .bind(project_id.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(pm_internal)?;
        let row = sqlx::query_as::<_, ItemRow>(
            "INSERT INTO proj_work_item (id, project_id, item_number, item_type, title, \
             description, status, priority, assignee, reporter, custom_fields) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
             RETURNING id, project_id, item_number, item_type, title, description, status, \
             priority, assignee, reporter, custom_fields, created_at, updated_at",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(project_id.as_uuid())
        .bind(number)
        .bind(item.item_type.as_str())
        .bind(&item.title)
        .bind(&item.description)
        .bind(initial_status)
        .bind(item.priority.map(|p| p.as_str()).unwrap_or("medium"))
        .bind(item.assignee.map(|u| u.as_uuid()))
        .bind(reporter.map(|u| u.as_uuid()))
        .bind(&item.custom_fields)
        .fetch_one(&mut *tx)
        .await
        .map_err(unique_conflict)?;
        tx.commit().await.map_err(pm_internal)?;
        row.try_into()
    }

    async fn find_by_id(
        &self,
        id: crate::domain::WorkItemId,
    ) -> Result<Option<WorkItem>, AppError> {
        let sql = format!("SELECT {ITEM_COLS} FROM proj_work_item WHERE id = $1");
        let row = sqlx::query_as::<_, ItemRow>(&sql)
            .bind(id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(pm_internal)?;
        row.map(WorkItem::try_from).transpose()
    }

    async fn list(
        &self,
        project_id: ProjectId,
        item_type: Option<ItemType>,
        status: Option<&str>,
        assignee: Option<UserId>,
    ) -> Result<Vec<WorkItem>, AppError> {
        let sql = format!(
            "SELECT {ITEM_COLS} FROM proj_work_item WHERE project_id = $1 \
             AND ($2::varchar IS NULL OR item_type = $2) \
             AND ($3::varchar IS NULL OR status = $3) \
             AND ($4::uuid IS NULL OR assignee = $4) \
             ORDER BY item_number DESC"
        );
        let rows = sqlx::query_as::<_, ItemRow>(&sql)
            .bind(project_id.as_uuid())
            .bind(item_type.map(|t| t.as_str()))
            .bind(status)
            .bind(assignee.map(|u| u.as_uuid()))
            .fetch_all(&self.pool)
            .await
            .map_err(pm_internal)?;
        rows.into_iter().map(WorkItem::try_from).collect()
    }

    async fn update_fields(
        &self,
        id: crate::domain::WorkItemId,
        title: Option<&str>,
        description: Option<&str>,
        priority: Option<Priority>,
        assignee: Option<Option<UserId>>,
        custom_fields: Option<&serde_json::Value>,
    ) -> Result<WorkItem, AppError> {
        let sql = format!(
            "UPDATE proj_work_item SET \
             title = COALESCE($2, title), \
             description = COALESCE($3, description), \
             priority = COALESCE($4, priority), \
             assignee = COALESCE($5, assignee), \
             custom_fields = COALESCE($6, custom_fields), \
             updated_at = now() \
             WHERE id = $1 RETURNING {ITEM_COLS}"
        );
        let row = sqlx::query_as::<_, ItemRow>(&sql)
            .bind(id.as_uuid())
            .bind(title)
            .bind(description)
            .bind(priority.map(|p| p.as_str()))
            .bind(assignee.map(|u| u.map(|x| x.as_uuid())))
            .bind(custom_fields)
            .fetch_optional(&self.pool)
            .await
            .map_err(pm_internal)?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "工作项不存在"))?;
        row.try_into()
    }

    async fn update_status(
        &self,
        id: crate::domain::WorkItemId,
        status: &str,
    ) -> Result<WorkItem, AppError> {
        let sql = format!(
            "UPDATE proj_work_item SET status = $2, updated_at = now() \
             WHERE id = $1 RETURNING {ITEM_COLS}"
        );
        let row = sqlx::query_as::<_, ItemRow>(&sql)
            .bind(id.as_uuid())
            .bind(status)
            .fetch_optional(&self.pool)
            .await
            .map_err(pm_internal)?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "工作项不存在"))?;
        row.try_into()
    }
}
