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
    parent_id: Option<uuid::Uuid>,
    board_order: i64,
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
            parent_id: r.parent_id.map(crate::domain::WorkItemId::from_uuid),
            board_order: r.board_order,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

const ITEM_COLS: &str = "id, project_id, item_number, item_type, title, description, status, \
     priority, assignee, reporter, custom_fields, parent_id, board_order, created_at, updated_at";

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
             description, status, priority, assignee, reporter, custom_fields, parent_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
             RETURNING id, project_id, item_number, item_type, title, description, status, \
             priority, assignee, reporter, custom_fields, parent_id, board_order, \
             created_at, updated_at",
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
        .bind(item.parent_id.map(|p| p.as_uuid()))
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

    async fn set_sprint(
        &self,
        id: crate::domain::WorkItemId,
        sprint_id: Option<crate::domain::SprintId>,
    ) -> Result<(), AppError> {
        sqlx::query("UPDATE proj_work_item SET sprint_id = $2, updated_at = now() WHERE id = $1")
            .bind(id.as_uuid())
            .bind(sprint_id.map(|s| s.as_uuid()))
            .execute(&self.pool)
            .await
            .map_err(pm_internal)?;
        Ok(())
    }

    async fn set_milestone(
        &self,
        id: crate::domain::WorkItemId,
        milestone_id: Option<crate::domain::MilestoneId>,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE proj_work_item SET milestone_id = $2, updated_at = now() WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(milestone_id.map(|m| m.as_uuid()))
        .execute(&self.pool)
        .await
        .map_err(pm_internal)?;
        Ok(())
    }

    async fn list_for_user(&self, user: UserId) -> Result<Vec<WorkItem>, AppError> {
        let sql = format!(
            "SELECT {ITEM_COLS} FROM proj_work_item \
             WHERE assignee = $1 OR reporter = $1 ORDER BY updated_at DESC LIMIT 100"
        );
        let rows = sqlx::query_as::<_, ItemRow>(&sql)
            .bind(user.as_uuid())
            .fetch_all(&self.pool)
            .await
            .map_err(pm_internal)?;
        rows.into_iter().map(WorkItem::try_from).collect()
    }

    async fn set_board_positions(
        &self,
        positions: &[(crate::domain::WorkItemId, String, i64)],
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(pm_internal)?;
        for (id, status, order) in positions {
            sqlx::query(
                "UPDATE proj_work_item SET status = $2, board_order = $3, updated_at = now() \
                 WHERE id = $1",
            )
            .bind(id.as_uuid())
            .bind(status)
            .bind(order)
            .execute(&mut *tx)
            .await
            .map_err(pm_internal)?;
        }
        tx.commit().await.map_err(pm_internal)?;
        Ok(())
    }
}

/// 迭代仓储 PG 实现。
#[derive(Clone)]
pub struct PgSprintRepository {
    pool: PgPool,
}

impl PgSprintRepository {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    #[allow(clippy::too_many_arguments)]
    fn row_into(
        project_id: ProjectId,
        id: uuid::Uuid,
        name: String,
        goal: String,
        starts_at: Option<DateTime<Utc>>,
        ends_at: Option<DateTime<Utc>>,
        capacity_points: i32,
        status: String,
        created_at: DateTime<Utc>,
    ) -> crate::domain::Sprint {
        crate::domain::Sprint {
            id: crate::domain::SprintId::from_uuid(id),
            project_id,
            name,
            goal,
            starts_at,
            ends_at,
            capacity_points,
            status: crate::domain::SprintStatus::from_str_opt(&status)
                .unwrap_or(crate::domain::SprintStatus::Planned),
            created_at,
        }
    }
}

#[async_trait]
impl crate::domain::SprintRepository for PgSprintRepository {
    async fn create(
        &self,
        project_id: ProjectId,
        sprint: &crate::domain::NewSprint,
    ) -> Result<crate::domain::Sprint, AppError> {
        let (id, created_at): (uuid::Uuid, DateTime<Utc>) = sqlx::query_as(
            "INSERT INTO proj_sprint (id, project_id, name, goal, starts_at, ends_at, capacity_points) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id, created_at",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(project_id.as_uuid())
        .bind(&sprint.name)
        .bind(&sprint.goal)
        .bind(sprint.starts_at)
        .bind(sprint.ends_at)
        .bind(sprint.capacity_points)
        .fetch_one(&self.pool)
        .await
        .map_err(pm_internal)?;
        Ok(Self::row_into(
            project_id,
            id,
            sprint.name.clone(),
            sprint.goal.clone(),
            sprint.starts_at,
            sprint.ends_at,
            sprint.capacity_points,
            "planned".to_owned(),
            created_at,
        ))
    }

    async fn list_by_project(
        &self,
        project_id: ProjectId,
    ) -> Result<Vec<crate::domain::Sprint>, AppError> {
        let rows: Vec<(
            uuid::Uuid,
            String,
            String,
            Option<DateTime<Utc>>,
            Option<DateTime<Utc>>,
            i32,
            String,
            DateTime<Utc>,
        )> = sqlx::query_as(
            "SELECT id, name, goal, starts_at, ends_at, capacity_points, status, created_at \
                 FROM proj_sprint WHERE project_id = $1 ORDER BY created_at DESC",
        )
        .bind(project_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(pm_internal)?;
        Ok(rows
            .into_iter()
            .map(|(id, name, goal, sa, ea, cp, st, ca)| {
                Self::row_into(project_id, id, name, goal, sa, ea, cp, st, ca)
            })
            .collect())
    }

    async fn find_by_id(
        &self,
        id: crate::domain::SprintId,
    ) -> Result<Option<crate::domain::Sprint>, AppError> {
        let rows: Vec<(uuid::Uuid, uuid::Uuid, String, String, Option<DateTime<Utc>>, Option<DateTime<Utc>>, i32, String, DateTime<Utc>)> =
            sqlx::query_as(
                "SELECT id, project_id, name, goal, starts_at, ends_at, capacity_points, status, created_at \
                 FROM proj_sprint WHERE id = $1",
            )
            .bind(id.as_uuid())
            .fetch_all(&self.pool)
            .await
            .map_err(pm_internal)?;
        Ok(rows
            .into_iter()
            .next()
            .map(|(id, project_id, name, goal, sa, ea, cp, st, ca)| {
                Self::row_into(
                    ProjectId::from_uuid(project_id),
                    id,
                    name,
                    goal,
                    sa,
                    ea,
                    cp,
                    st,
                    ca,
                )
            }))
    }

    async fn update_status(
        &self,
        id: crate::domain::SprintId,
        status: crate::domain::SprintStatus,
    ) -> Result<(), AppError> {
        sqlx::query("UPDATE proj_sprint SET status = $2, updated_at = now() WHERE id = $1")
            .bind(id.as_uuid())
            .bind(status.as_str())
            .execute(&self.pool)
            .await
            .map_err(pm_internal)?;
        Ok(())
    }

    async fn demote_active(&self, project_id: ProjectId) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE proj_sprint SET status = 'planned', updated_at = now() \
             WHERE project_id = $1 AND status = 'active'",
        )
        .bind(project_id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(pm_internal)?;
        Ok(())
    }

    async fn progress(
        &self,
        id: crate::domain::SprintId,
    ) -> Result<crate::domain::SprintProgress, AppError> {
        let (total, done, points): (i64, i64, Option<i64>) = sqlx::query_as(
            "SELECT count(*), count(*) FILTER (WHERE status = 'done'), \
             SUM(COALESCE((custom_fields->>'story_points')::int, 0)) \
             FROM proj_work_item WHERE sprint_id = $1",
        )
        .bind(id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(pm_internal)?;
        Ok(crate::domain::SprintProgress {
            total,
            done,
            committed_points: points.unwrap_or(0),
        })
    }
}

/// 里程碑仓储 PG 实现。
#[derive(Clone)]
pub struct PgMilestoneRepository {
    pool: PgPool,
}

impl PgMilestoneRepository {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    fn row_into(
        project_id: ProjectId,
        id: uuid::Uuid,
        name: String,
        description: String,
        due_date: Option<chrono::NaiveDate>,
        status: String,
        created_at: DateTime<Utc>,
    ) -> Result<crate::domain::Milestone, AppError> {
        Ok(crate::domain::Milestone {
            id: crate::domain::MilestoneId::from_uuid(id),
            project_id,
            name,
            description,
            due_date: due_date
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|dt| dt.and_utc()),
            status,
            created_at,
        })
    }
}

#[async_trait]
impl crate::domain::MilestoneRepository for PgMilestoneRepository {
    async fn create(
        &self,
        project_id: ProjectId,
        milestone: &crate::domain::NewMilestone,
    ) -> Result<crate::domain::Milestone, AppError> {
        let (id, created_at): (uuid::Uuid, DateTime<Utc>) = sqlx::query_as(
            "INSERT INTO proj_milestone (id, project_id, name, description, due_date) \
             VALUES ($1, $2, $3, $4, $5) RETURNING id, created_at",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(project_id.as_uuid())
        .bind(&milestone.name)
        .bind(&milestone.description)
        .bind(milestone.due_date.map(|d| d.date_naive()))
        .fetch_one(&self.pool)
        .await
        .map_err(pm_internal)?;
        Self::row_into(
            project_id,
            id,
            milestone.name.clone(),
            milestone.description.clone(),
            milestone.due_date.map(|d| d.date_naive()),
            "open".to_owned(),
            created_at,
        )
    }

    async fn list_by_project(
        &self,
        project_id: ProjectId,
    ) -> Result<Vec<crate::domain::Milestone>, AppError> {
        let rows: Vec<(
            uuid::Uuid,
            String,
            String,
            Option<chrono::NaiveDate>,
            String,
            DateTime<Utc>,
        )> = sqlx::query_as(
            "SELECT id, name, description, due_date, status, created_at \
                 FROM proj_milestone WHERE project_id = $1 ORDER BY created_at",
        )
        .bind(project_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(pm_internal)?;
        rows.into_iter()
            .map(|(id, name, desc, due, st, ca)| {
                Self::row_into(project_id, id, name, desc, due, st, ca)
            })
            .collect()
    }

    async fn find_by_id(
        &self,
        id: crate::domain::MilestoneId,
    ) -> Result<Option<crate::domain::Milestone>, AppError> {
        let rows: Vec<(
            uuid::Uuid,
            uuid::Uuid,
            String,
            String,
            Option<chrono::NaiveDate>,
            String,
            DateTime<Utc>,
        )> = sqlx::query_as(
            "SELECT id, project_id, name, description, due_date, status, created_at \
                 FROM proj_milestone WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(pm_internal)?;
        rows.into_iter()
            .next()
            .map(|(id, project_id, name, desc, due, st, ca)| {
                Self::row_into(
                    ProjectId::from_uuid(project_id),
                    id,
                    name,
                    desc,
                    due,
                    st,
                    ca,
                )
            })
            .transpose()
    }

    async fn close(&self, id: crate::domain::MilestoneId) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE proj_milestone SET status = 'closed', updated_at = now() WHERE id = $1",
        )
        .bind(id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(pm_internal)?;
        Ok(())
    }

    async fn progress(
        &self,
        id: crate::domain::MilestoneId,
    ) -> Result<crate::domain::MilestoneProgress, AppError> {
        let (total, done): (i64, i64) = sqlx::query_as(
            "SELECT count(*), count(*) FILTER (WHERE status = 'done') \
             FROM proj_work_item WHERE milestone_id = $1",
        )
        .bind(id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(pm_internal)?;
        Ok(crate::domain::MilestoneProgress { total, done })
    }
}
