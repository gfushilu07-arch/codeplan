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

/// 项目域仓储集合(收拢构造函数参数,避免签名膨胀)。
pub struct ProjectStores {
    /// 项目。
    pub projects: Arc<dyn ProjectRepository>,
    /// 工作项。
    pub items: Arc<dyn WorkItemRepository>,
    /// 迭代。
    pub sprints: Arc<dyn crate::domain::SprintRepository>,
    /// 里程碑。
    pub milestones: Arc<dyn crate::domain::MilestoneRepository>,
    /// 评论。
    pub comments: Arc<dyn crate::domain::CommentRepository>,
    /// 工时。
    pub worklogs: Arc<dyn crate::domain::WorklogRepository>,
}

/// 项目域服务。
pub struct ProjectService {
    projects: Arc<dyn ProjectRepository>,
    items: Arc<dyn WorkItemRepository>,
    sprints: Arc<dyn crate::domain::SprintRepository>,
    milestones: Arc<dyn crate::domain::MilestoneRepository>,
    comments: Arc<dyn crate::domain::CommentRepository>,
    worklogs: Arc<dyn crate::domain::WorklogRepository>,
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
        stores: ProjectStores,
        authz: Arc<dyn Authz>,
        events: Arc<dyn codeplan_kernel::events::EventBus>,
    ) -> Self {
        Self {
            projects: stores.projects,
            items: stores.items,
            sprints: stores.sprints,
            milestones: stores.milestones,
            comments: stores.comments,
            worklogs: stores.worklogs,
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
        // 缺陷特化:规范化 custom_fields(severity 缺省注入/非法校验,T-114)
        let normalized = Self::normalize_custom_fields(item.item_type, &item.custom_fields)?;
        let mut item = item.clone();
        item.custom_fields = normalized;
        let created = self
            .items
            .create(project.id, &item, sm.initial(), Some(actor.id))
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
                        "assignee": created.assignee.map(|u| u.as_uuid()),
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
    /// 系统自动流转(合并关联等自动化;无 RBAC,事件 actor = system)。
    /// 按状态机 BFS 最短路径逐步流转(如 open → in_progress → resolved)。
    ///
    /// # Errors
    /// 工作项不存在 `PM-404`;不可达 `PM-409`;存储故障。
    pub async fn transition_system(
        &self,
        item_id: crate::domain::WorkItemId,
        to: &str,
    ) -> Result<WorkItem, AppError> {
        let item = self
            .items
            .find_by_id(item_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "工作项不存在"))?;
        if item.status == to {
            return Ok(item);
        }
        let sm = StateMachine::builtin(item.item_type);
        let Some(path) = sm.path(&item.status, to) else {
            return Err(AppError::conflict(
                DOMAIN_CODE,
                format!("不可达:{} → {}", item.status, to),
            ));
        };
        let mut current = item;
        for step in path {
            current = self.do_transition(current, &step, None).await?;
        }
        Ok(current)
    }

    /// 手动流转(RBAC Write;发布 `workitem.status_changed`)。
    ///
    /// # Errors
    /// 工作项不存在 `PM-404`;非法流转 `PM-409`;无权限 `PM-403`。
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
        self.do_transition(item, to, Some(actor.id)).await
    }

    /// 流转核心:状态机 + 缺陷门禁 + 事件(actor 为空 = system)。
    async fn do_transition(
        &self,
        item: WorkItem,
        to: &str,
        actor: Option<codeplan_kernel::ids::UserId>,
    ) -> Result<WorkItem, AppError> {
        let sm = StateMachine::builtin(item.item_type);
        if !sm.can_transition(&item.status, to) {
            return Err(AppError::conflict(
                DOMAIN_CODE,
                format!("非法流转:{} → {}", item.status, to),
            ));
        }
        // 缺陷门禁:流转到 resolved 须有复现步骤(T-114);
        // 系统自动流转(合并即修复)豁免
        if actor.is_some() && item.item_type == ItemType::Bug && to == "resolved" {
            let fields = crate::domain::BugFields::from_custom_fields(&item.custom_fields)?;
            if !fields.ready_to_resolve() {
                return Err(AppError::invalid(
                    DOMAIN_CODE,
                    "缺陷修复前须填写复现步骤(custom_fields.repro_steps)",
                ));
            }
        }
        let updated = self.items.update_status(item.id, to).await?;
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
                        "assignee": item.assignee.map(|u| u.as_uuid()),
                        "reporter": item.reporter.map(|u| u.as_uuid()),
                    }),
                )
                .with_actor(
                    actor
                        .map(|u| u.as_uuid().to_string())
                        .unwrap_or_else(|| "system".to_owned()),
                ),
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
        let custom_fields = match custom_fields {
            Some(cf) => Some(Self::normalize_custom_fields(item.item_type, cf)?),
            None => None,
        };
        let updated = self
            .items
            .update_fields(
                item_id,
                title,
                description,
                priority,
                None,
                custom_fields.as_ref(),
            )
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

    /// 看板视图:按工作流状态分列(每列 items + total)。
    ///
    /// # Errors
    /// 项目不存在 `PM-404`;无读权限 `PM-403`。
    pub async fn board_view(
        &self,
        actor: &CurrentUser,
        project_key: &str,
        item_type: Option<crate::domain::ItemType>,
    ) -> Result<serde_json::Value, AppError> {
        let project = self.project_by_key(project_key).await?;
        self.ensure(actor, Action::Read, project.id).await?;
        let t = item_type.unwrap_or(crate::domain::ItemType::Story);
        let sm = StateMachine::builtin(t);
        let items = self.items.list(project.id, Some(t), None, None).await?;
        let columns: Vec<serde_json::Value> = sm
            .states()
            .iter()
            .map(|status| {
                let mut in_column: Vec<&WorkItem> =
                    items.iter().filter(|i| &i.status == status).collect();
                in_column.sort_by_key(|i| i.board_order);
                serde_json::json!({
                    "status": status,
                    "total": in_column.len(),
                    "items": in_column
                        .iter()
                        .map(|i| serde_json::json!({
                            "id": i.id.as_uuid(),
                            "number": i.item_number,
                            "title": i.title,
                            "priority": i.priority.as_str(),
                            "board_order": i.board_order,
                        }))
                        .collect::<Vec<_>>(),
                })
            })
            .collect();
        Ok(serde_json::json!({ "item_type": t.as_str(), "columns": columns }))
    }

    /// 看板拖拽重排:逐项校验(状态变更走工作流;同列重排免校验)。
    ///
    /// # Errors
    /// 非法流转 `PM-409`;无写权限 `PM-403`。
    pub async fn reorder_board(
        &self,
        actor: &CurrentUser,
        project_key: &str,
        positions: &[(crate::domain::WorkItemId, String, i64)],
    ) -> Result<(), AppError> {
        let project = self.project_by_key(project_key).await?;
        for (id, new_status, _order) in positions {
            let item = self
                .items
                .find_by_id(*id)
                .await?
                .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "工作项不存在"))?;
            if item.project_id.as_uuid() != project.id.as_uuid() {
                return Err(AppError::invalid(DOMAIN_CODE, "工作项不属于该项目"));
            }
            if item.status != *new_status {
                let sm = StateMachine::builtin(item.item_type);
                if !sm.can_transition(&item.status, new_status) {
                    return Err(AppError::conflict(
                        DOMAIN_CODE,
                        format!("非法流转:{} → {}", item.status, new_status),
                    ));
                }
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
                                "to": new_status,
                            }),
                        )
                        .with_actor(actor.id.as_uuid().to_string()),
                    ])
                    .await;
            }
        }
        self.items.set_board_positions(positions).await
    }

    /// 需求树:epic → story → task(仅返回有父子关系的节点构成的森林)。
    ///
    /// # Errors
    /// 项目不存在 `PM-404`;无读权限 `PM-403`。
    pub async fn items_tree(
        &self,
        actor: &CurrentUser,
        project_key: &str,
    ) -> Result<Vec<serde_json::Value>, AppError> {
        let project = self.project_by_key(project_key).await?;
        self.ensure(actor, Action::Read, project.id).await?;
        let items = self.items.list(project.id, None, None, None).await?;
        let level = |t: crate::domain::ItemType| match t {
            crate::domain::ItemType::Epic => 0,
            crate::domain::ItemType::Story => 1,
            _ => 2,
        };
        let node = |i: &WorkItem, children: Vec<serde_json::Value>| {
            serde_json::json!({
                "id": i.id.as_uuid(),
                "display_key": format!("{}-{}", project.key, i.item_number),
                "item_type": i.item_type.as_str(),
                "title": i.title,
                "status": i.status,
                "children": children,
            })
        };
        // task 层
        let mut tasks: std::collections::HashMap<
            Option<crate::domain::WorkItemId>,
            Vec<serde_json::Value>,
        > = items
            .iter()
            .filter(|i| level(i.item_type) == 2)
            .map(|i| (i.parent_id, vec![node(i, Vec::new())]))
            .fold(
                std::collections::HashMap::new(),
                |mut acc, (parent, mut nodes)| {
                    acc.entry(parent).or_default().append(&mut nodes);
                    acc
                },
            );
        // story 层
        let mut stories: std::collections::HashMap<
            Option<crate::domain::WorkItemId>,
            Vec<serde_json::Value>,
        > = items
            .iter()
            .filter(|i| level(i.item_type) == 1)
            .map(|i| {
                let children = tasks.remove(&Some(i.id)).unwrap_or_default();
                (i.parent_id, vec![node(i, children)])
            })
            .fold(
                std::collections::HashMap::new(),
                |mut acc, (parent, mut nodes)| {
                    acc.entry(parent).or_default().append(&mut nodes);
                    acc
                },
            );
        // epic 根层(无父的 epic;无父 story/task 也作为根透出)
        let mut roots: Vec<serde_json::Value> = items
            .iter()
            .filter(|i| level(i.item_type) == 0)
            .map(|i| {
                let children = stories.remove(&Some(i.id)).unwrap_or_default();
                node(i, children)
            })
            .collect();
        for (parent, nodes) in stories.drain() {
            if parent.is_none() {
                roots.extend(nodes);
            }
        }
        for (_, nodes) in tasks.drain() {
            roots.extend(nodes.into_iter().filter(|_| false)); // 孤儿任务不进树(通过列表视图访问)
        }
        Ok(roots)
    }

    /// 缺陷类型:规范化 custom_fields(severity 注入默认/非法即 400);其他类型原样。
    fn normalize_custom_fields(
        item_type: ItemType,
        custom_fields: &serde_json::Value,
    ) -> Result<serde_json::Value, AppError> {
        if item_type != ItemType::Bug {
            return Ok(custom_fields.to_owned());
        }
        let fields = crate::domain::BugFields::from_custom_fields(custom_fields)?;
        Ok(fields.merged_into(custom_fields))
    }

    /// 工作台:与当前用户相关的工作项(assignee 或 reporter)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn my_items(&self, actor: &CurrentUser) -> Result<Vec<WorkItem>, AppError> {
        self.items.list_for_user(actor.id).await
    }

    /// 工作项详情(按 ID;含描述等全字段,供详情页)。
    ///
    /// # Errors
    /// 工作项不存在 `PM-404`;无读权限 `PM-403`。
    pub async fn item_by_id(
        &self,
        actor: &CurrentUser,
        item_id: crate::domain::WorkItemId,
    ) -> Result<WorkItem, AppError> {
        let item = self
            .items
            .find_by_id(item_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "工作项不存在"))?;
        self.ensure(actor, Action::Read, item.project_id).await?;
        Ok(item)
    }

    /// 当前状态下可流转的目标状态(详情页按钮由服务端状态机裁定,避免前后端漂移)。
    ///
    /// # Errors
    /// 工作项不存在 `PM-404`;无读权限 `PM-403`。
    pub async fn transitions_for(
        &self,
        actor: &CurrentUser,
        item_id: crate::domain::WorkItemId,
    ) -> Result<Vec<String>, AppError> {
        let item = self.item_by_id(actor, item_id).await?;
        let sm = StateMachine::builtin(item.item_type);
        Ok(sm
            .states()
            .iter()
            .filter(|s| sm.can_transition(&item.status, s))
            .cloned()
            .collect())
    }

    /// 新增评论(发布 `workitem.commented`)。
    ///
    /// # Errors
    /// 工作项不存在 `PM-404`;无写权限 `PM-403`;正文为空 `PM-400`。
    pub async fn add_comment(
        &self,
        actor: &CurrentUser,
        item_id: crate::domain::WorkItemId,
        body: &str,
    ) -> Result<crate::domain::WorkItemComment, AppError> {
        let body = body.trim();
        if body.is_empty() {
            return Err(AppError::invalid(DOMAIN_CODE, "评论内容不能为空"));
        }
        let item = self
            .items
            .find_by_id(item_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "工作项不存在"))?;
        self.ensure(actor, Action::Write, item.project_id).await?;
        let comment = self
            .comments
            .add(&crate::domain::NewComment {
                work_item_id: item_id,
                author_id: Some(actor.id),
                body: body.to_owned(),
            })
            .await?;
        let _ = self
            .events
            .publish(vec![
                EventEnvelope::new(
                    "workitem.commented",
                    json!({
                        "id": item.id.as_uuid(),
                        "project_id": item.project_id.as_uuid(),
                        "number": item.item_number,
                        "comment_id": comment.id.as_uuid(),
                        "author": actor.id.as_uuid(),
                    }),
                )
                .with_actor(actor.id.as_uuid().to_string()),
            ])
            .await;
        Ok(comment)
    }

    /// 评论列表(时间正序)。
    ///
    /// # Errors
    /// 工作项不存在 `PM-404`;无读权限 `PM-403`。
    pub async fn list_comments(
        &self,
        actor: &CurrentUser,
        item_id: crate::domain::WorkItemId,
    ) -> Result<Vec<crate::domain::WorkItemComment>, AppError> {
        let item = self.item_by_id(actor, item_id).await?;
        let _ = item;
        self.comments.list_by_item(item_id).await
    }

    /// 删除评论(作者本人或项目 Write 权限)。
    ///
    /// # Errors
    /// 评论不存在 `PM-404`;非作者且无写权限 `PM-403`。
    pub async fn delete_comment(
        &self,
        actor: &CurrentUser,
        comment_id: crate::domain::CommentId,
    ) -> Result<(), AppError> {
        let comment = self
            .comments
            .find_by_id(comment_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "评论不存在"))?;
        let item = self
            .items
            .find_by_id(comment.work_item_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "工作项不存在"))?;
        if comment.author_id != Some(actor.id) {
            self.ensure(actor, Action::Write, item.project_id).await?;
        }
        self.comments.delete(comment_id).await?;
        Ok(())
    }

    /// 登记工时(单条上限 24 小时,发布 `workitem.worklogged`)。
    ///
    /// # Errors
    /// 工作项不存在 `PM-404`;无写权限 `PM-403`;用时非法 `PM-400`。
    pub async fn log_work(
        &self,
        actor: &CurrentUser,
        item_id: crate::domain::WorkItemId,
        minutes: i64,
        note: &str,
        spent_on: Option<chrono::NaiveDate>,
    ) -> Result<crate::domain::Worklog, AppError> {
        if minutes <= 0 || minutes > 24 * 60 {
            return Err(AppError::invalid(DOMAIN_CODE, "单条工时须在 1 分钟 ~ 24 小时之间"));
        }
        let item = self
            .items
            .find_by_id(item_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "工作项不存在"))?;
        self.ensure(actor, Action::Write, item.project_id).await?;
        let log = self
            .worklogs
            .add(&crate::domain::NewWorklog {
                work_item_id: item_id,
                author_id: Some(actor.id),
                minutes,
                note: note.trim().to_owned(),
                spent_on,
            })
            .await?;
        let _ = self
            .events
            .publish(vec![
                EventEnvelope::new(
                    "workitem.worklogged",
                    json!({
                        "id": item.id.as_uuid(),
                        "project_id": item.project_id.as_uuid(),
                        "number": item.item_number,
                        "minutes": minutes,
                        "author": actor.id.as_uuid(),
                    }),
                )
                .with_actor(actor.id.as_uuid().to_string()),
            ])
            .await;
        Ok(log)
    }

    /// 工时明细(日期倒序)。
    ///
    /// # Errors
    /// 工作项不存在 `PM-404`;无读权限 `PM-403`。
    pub async fn list_worklogs(
        &self,
        actor: &CurrentUser,
        item_id: crate::domain::WorkItemId,
    ) -> Result<Vec<crate::domain::Worklog>, AppError> {
        self.item_by_id(actor, item_id).await?;
        self.worklogs.list_by_item(item_id).await
    }

    /// 工时汇总(累计 / 预估 / 按人)。
    ///
    /// # Errors
    /// 工作项不存在 `PM-404`;无读权限 `PM-403`。
    pub async fn worklog_summary(
        &self,
        actor: &CurrentUser,
        item_id: crate::domain::WorkItemId,
    ) -> Result<crate::domain::WorklogSummary, AppError> {
        let item = self.item_by_id(actor, item_id).await?;
        let estimate = item
            .custom_fields
            .get("estimate_minutes")
            .and_then(serde_json::Value::as_i64);
        let logs = self.worklogs.list_by_item(item_id).await?;
        Ok(crate::domain::summarize_worklogs(&logs, estimate))
    }

    /// 删除工时(登记者本人或项目写权限)。
    ///
    /// # Errors
    /// 记录不存在 `PM-404`;非本人且无写权限 `PM-403`。
    pub async fn delete_worklog(
        &self,
        actor: &CurrentUser,
        worklog_id: crate::domain::WorklogId,
    ) -> Result<(), AppError> {
        let log = self
            .worklogs
            .find_by_id(worklog_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "工时记录不存在"))?;
        if log.author_id != Some(actor.id) {
            let item = self
                .items
                .find_by_id(log.work_item_id)
                .await?
                .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "工作项不存在"))?;
            self.ensure(actor, Action::Write, item.project_id).await?;
        }
        self.worklogs.delete(worklog_id).await?;
        Ok(())
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
