//! 项目域用例:项目/工作项 CRUD、状态流转(引擎校验)、领域事件发布。

use std::sync::Arc;

use codeplan_common::error::AppError;
use codeplan_kernel::authz::{Action, Authz};
use codeplan_kernel::events::EventEnvelope;
use codeplan_kernel::identity::CurrentUser;
use serde_json::json;

use crate::DOMAIN_CODE;
use crate::domain::{
    ItemType, NewProject, NewWorkItem, Project, ProjectId, ProjectRepository, WorkItem,
    WorkItemRepository,
};
use crate::workflow::StateMachine;

/// 项目域服务。
pub struct ProjectService {
    projects: Arc<dyn ProjectRepository>,
    items: Arc<dyn WorkItemRepository>,
    sprints: Arc<dyn crate::domain::SprintRepository>,
    milestones: Arc<dyn crate::domain::MilestoneRepository>,
    authz: Arc<dyn Authz>,
    events: Arc<dyn codeplan_kernel::events::EventBus>,
}

/// 项目键规则:大写字母/数字/连字符,2~16 位。
fn valid_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    (2..=16).contains(&bytes.len())
        && bytes
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'-')
}

impl ProjectService {
    /// 构造。
    #[must_use]
    pub fn new(
        projects: Arc<dyn ProjectRepository>,
        items: Arc<dyn WorkItemRepository>,
        sprints: Arc<dyn crate::domain::SprintRepository>,
        milestones: Arc<dyn crate::domain::MilestoneRepository>,
        authz: Arc<dyn Authz>,
        events: Arc<dyn codeplan_kernel::events::EventBus>,
    ) -> Self {
        Self {
            projects,
            items,
            sprints,
            milestones,
            authz,
            events,
        }
    }

    /// 创建迭代。
    ///
    /// # Errors
    /// 项目不存在 `PM-404`;无管理权限 `PM-403`。
    pub async fn create_sprint(
        &self,
        actor: &CurrentUser,
        project_key: &str,
        sprint: &crate::domain::NewSprint,
    ) -> Result<crate::domain::Sprint, AppError> {
        let project = self.project_by_key(project_key).await?;
        self.ensure(actor, Action::Admin, project.id).await?;
        self.sprints.create(project.id, sprint).await
    }

    /// 迭代列表(含进度)。
    ///
    /// # Errors
    /// 项目不存在 `PM-404`;无读权限 `PM-403`。
    pub async fn list_sprints(
        &self,
        actor: &CurrentUser,
        project_key: &str,
    ) -> Result<Vec<(crate::domain::Sprint, crate::domain::SprintProgress)>, AppError> {
        let project = self.project_by_key(project_key).await?;
        self.ensure(actor, Action::Read, project.id).await?;
        let sprints = self.sprints.list_by_project(project.id).await?;
        let mut out = Vec::new();
        for s in sprints {
            let progress = self.sprints.progress(s.id).await?;
            out.push((s, progress));
        }
        Ok(out)
    }

    /// 启动迭代(同项目其他 active 迭代自动回到 planned)。
    ///
    /// # Errors
    /// 迭代不存在 `PM-404`;已完成不可启动 `PM-409`;无管理权限 `PM-403`。
    pub async fn start_sprint(
        &self,
        actor: &CurrentUser,
        sprint_id: crate::domain::SprintId,
    ) -> Result<(), AppError> {
        let sprint = self
            .sprints
            .find_by_id(sprint_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "迭代不存在"))?;
        self.ensure(actor, Action::Admin, sprint.project_id).await?;
        if sprint.status == crate::domain::SprintStatus::Completed {
            return Err(AppError::conflict(DOMAIN_CODE, "已完成的迭代不可再启动"));
        }
        self.sprints.demote_active(sprint.project_id).await?;
        self.sprints
            .update_status(sprint_id, crate::domain::SprintStatus::Active)
            .await
    }

    /// 完成迭代。
    ///
    /// # Errors
    /// 迭代不存在 `PM-404`;未启动 `PM-409`;无管理权限 `PM-403`。
    pub async fn complete_sprint(
        &self,
        actor: &CurrentUser,
        sprint_id: crate::domain::SprintId,
    ) -> Result<(), AppError> {
        let sprint = self
            .sprints
            .find_by_id(sprint_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "迭代不存在"))?;
        self.ensure(actor, Action::Admin, sprint.project_id).await?;
        if sprint.status != crate::domain::SprintStatus::Active {
            return Err(AppError::conflict(DOMAIN_CODE, "仅进行中的迭代可完成"));
        }
        self.sprints
            .update_status(sprint_id, crate::domain::SprintStatus::Completed)
            .await?;
        self.audit_event(actor, "sprint.completed", sprint.id.as_uuid())
            .await;
        Ok(())
    }

    /// 工作项关联迭代。
    ///
    /// # Errors
    /// 无写权限 `PM-403`。
    pub async fn set_item_sprint(
        &self,
        actor: &CurrentUser,
        item_id: crate::domain::WorkItemId,
        sprint_id: Option<crate::domain::SprintId>,
    ) -> Result<(), AppError> {
        let item = self
            .items
            .find_by_id(item_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "工作项不存在"))?;
        self.ensure(actor, Action::Write, item.project_id).await?;
        self.items.set_sprint(item_id, sprint_id).await
    }

    /// 创建里程碑。
    ///
    /// # Errors
    /// 项目不存在 `PM-404`;无管理权限 `PM-403`。
    pub async fn create_milestone(
        &self,
        actor: &CurrentUser,
        project_key: &str,
        milestone: &crate::domain::NewMilestone,
    ) -> Result<crate::domain::Milestone, AppError> {
        let project = self.project_by_key(project_key).await?;
        self.ensure(actor, Action::Admin, project.id).await?;
        self.milestones.create(project.id, milestone).await
    }

    /// 里程碑列表(含进度)。
    ///
    /// # Errors
    /// 项目不存在 `PM-404`;无读权限 `PM-403`。
    pub async fn list_milestones(
        &self,
        actor: &CurrentUser,
        project_key: &str,
    ) -> Result<Vec<(crate::domain::Milestone, crate::domain::MilestoneProgress)>, AppError> {
        let project = self.project_by_key(project_key).await?;
        self.ensure(actor, Action::Read, project.id).await?;
        let milestones = self.milestones.list_by_project(project.id).await?;
        let mut out = Vec::new();
        for m in milestones {
            let progress = self.milestones.progress(m.id).await?;
            out.push((m, progress));
        }
        Ok(out)
    }

    /// 关闭里程碑。
    ///
    /// # Errors
    /// 无管理权限 `PM-403`。
    pub async fn close_milestone(
        &self,
        actor: &CurrentUser,
        milestone_id: crate::domain::MilestoneId,
    ) -> Result<(), AppError> {
        let milestone = self
            .milestones
            .find_by_id(milestone_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "里程碑不存在"))?;
        self.ensure(actor, Action::Admin, milestone.project_id)
            .await?;
        self.milestones.close(milestone_id).await?;
        self.audit_event(actor, "milestone.closed", milestone_id.as_uuid())
            .await;
        Ok(())
    }

    /// 工作项关联里程碑。
    ///
    /// # Errors
    /// 无写权限 `PM-403`。
    pub async fn set_item_milestone(
        &self,
        actor: &CurrentUser,
        item_id: crate::domain::WorkItemId,
        milestone_id: Option<crate::domain::MilestoneId>,
    ) -> Result<(), AppError> {
        let item = self
            .items
            .find_by_id(item_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "工作项不存在"))?;
        self.ensure(actor, Action::Write, item.project_id).await?;
        self.items.set_milestone(item_id, milestone_id).await
    }

    async fn audit_event(&self, actor: &CurrentUser, action: &str, id: uuid::Uuid) {
        let _ = self
            .events
            .publish(vec![
                EventEnvelope::new(action, json!({"id": id, "actor": actor.id.as_uuid()}))
                    .with_actor(actor.id.as_uuid().to_string()),
            ])
            .await;
    }

    /// 创建项目(发布 `project.created`,identity 监听后授予创建者 admin)。
    ///
    /// # Errors
    /// key 非法/冲突返回 `PM-400`/`PM-409`。
    pub async fn create_project(
        &self,
        actor: &CurrentUser,
        key: &str,
        name: &str,
        description: &str,
    ) -> Result<Project, AppError> {
        if !valid_key(key) {
            return Err(AppError::invalid(
                DOMAIN_CODE,
                "项目键须为大写字母/数字/连字符,2~16 位",
            ));
        }
        let project = self
            .projects
            .create(&NewProject {
                key: key.to_owned(),
                name: name.to_owned(),
                description: description.to_owned(),
                lead: Some(actor.id),
            })
            .await?;
        let _ = self
            .events
            .publish(vec![
                EventEnvelope::new(
                    "project.created",
                    json!({
                        "project_id": project.id.as_uuid(),
                        "key": project.key,
                        "creator_id": actor.id.as_uuid(),
                    }),
                )
                .with_actor(actor.id.as_uuid().to_string()),
            ])
            .await;
        Ok(project)
    }

    /// 项目列表。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn list_projects(&self) -> Result<Vec<Project>, AppError> {
        self.projects.list().await
    }

    /// 按键取项目。
    ///
    /// # Errors
    /// 不存在返回 `PM-404`。
    pub async fn project_by_key(&self, key: &str) -> Result<Project, AppError> {
        self.projects
            .find_by_key(key)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "项目不存在"))
    }

    /// 创建工作项(需要项目写权限;发布 `workitem.created`)。
    ///
    /// # Errors
    /// 项目不存在 `PM-404`;无写权限 `PM-403`;存储故障。
    pub async fn create_item(
        &self,
        actor: &CurrentUser,
        project_key: &str,
        item: &NewWorkItem,
    ) -> Result<WorkItem, AppError> {
        let project = self.project_by_key(project_key).await?;
        self.ensure(actor, Action::Write, project.id).await?;
        let sm = StateMachine::builtin(item.item_type);
        let created = self
            .items
            .create(project.id, item, sm.initial(), Some(actor.id))
            .await?;
        let _ = self
            .events
            .publish(vec![
                EventEnvelope::new(
                    "workitem.created",
                    json!({
                        "id": created.id.as_uuid(),
                        "project_id": project.id.as_uuid(),
                        "number": created.item_number,
                        "type": created.item_type.as_str(),
                        "title": created.title,
                        "reporter": actor.id.as_uuid(),
                    }),
                )
                .with_actor(actor.id.as_uuid().to_string()),
            ])
            .await;
        Ok(created)
    }

    /// 按键列项目全部工作项(已认证即可访问;企业内网默认可见,细粒度读权限随项目私有化引入)。
    ///
    /// # Errors
    /// 项目不存在 `PM-404`。
    pub async fn list_items_public(&self, key: &str) -> Result<Vec<WorkItem>, AppError> {
        let project = self.project_by_key(key).await?;
        self.items.list(project.id, None, None, None).await
    }

    /// 工作项列表(需要项目读权限)。
    ///
    /// # Errors
    /// 项目不存在 `PM-404`;无权限 `PM-403`。
    pub async fn list_items(
        &self,
        actor: &CurrentUser,
        project_key: &str,
        item_type: Option<ItemType>,
        status: Option<&str>,
    ) -> Result<Vec<WorkItem>, AppError> {
        let project = self.project_by_key(project_key).await?;
        self.ensure(actor, Action::Read, project.id).await?;
        self.items.list(project.id, item_type, status, None).await
    }

    /// 状态流转(引擎校验;发布 `workitem.status_changed`)。
    ///
    /// # Errors
    /// 非法流转返回 `PM-409`;无权限 `PM-403`;不存在 `PM-404`。
    pub async fn transition(
        &self,
        actor: &CurrentUser,
        item_id: crate::domain::WorkItemId,
        to: &str,
    ) -> Result<WorkItem, AppError> {
        let item = self
            .items
            .find_by_id(item_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "工作项不存在"))?;
        self.ensure(actor, Action::Write, item.project_id).await?;
        let sm = StateMachine::builtin(item.item_type);
        if !sm.can_transition(&item.status, to) {
            return Err(AppError::conflict(
                DOMAIN_CODE,
                format!("非法流转:{} → {}", item.status, to),
            ));
        }
        let updated = self.items.update_status(item_id, to).await?;
        let _ = self
            .events
            .publish(vec![
                EventEnvelope::new(
                    "workitem.status_changed",
                    json!({
                        "id": item.id.as_uuid(),
                        "project_id": item.project_id.as_uuid(),
                        "number": item.item_number,
                        "from": item.status,
                        "to": to,
                    }),
                )
                .with_actor(actor.id.as_uuid().to_string()),
            ])
            .await;
        Ok(updated)
    }

    /// 更新可编辑字段(发布 `workitem.updated`)。
    ///
    /// # Errors
    /// 无权限 `PM-403`;不存在 `PM-404`。
    pub async fn update_item(
        &self,
        actor: &CurrentUser,
        item_id: crate::domain::WorkItemId,
        title: Option<&str>,
        description: Option<&str>,
        priority: Option<crate::domain::Priority>,
        custom_fields: Option<&serde_json::Value>,
    ) -> Result<WorkItem, AppError> {
        let item = self
            .items
            .find_by_id(item_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "工作项不存在"))?;
        self.ensure(actor, Action::Write, item.project_id).await?;
        let updated = self
            .items
            .update_fields(item_id, title, description, priority, None, custom_fields)
            .await?;
        let _ = self
            .events
            .publish(vec![
                EventEnvelope::new(
                    "workitem.updated",
                    json!({"id": item.id.as_uuid(), "number": item.item_number}),
                )
                .with_actor(actor.id.as_uuid().to_string()),
            ])
            .await;
        Ok(updated)
    }

    async fn ensure(
        &self,
        actor: &CurrentUser,
        action: Action,
        project_id: ProjectId,
    ) -> Result<(), AppError> {
        let locator = codeplan_kernel::authz::ResourceLocator::Project(project_id.as_uuid());
        self.authz
            .check(actor.id, action, &locator.to_ref())
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e)))?
            .then_some(())
            .ok_or_else(|| AppError::forbidden(DOMAIN_CODE, "无权限执行该操作"))
    }
}
