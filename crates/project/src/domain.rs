//! 项目管理域实体与仓储 trait(纯领域,不依赖 axum/sqlx)。

use chrono::{DateTime, Utc};
use codeplan_common::define_id;
use codeplan_common::error::AppError;
use codeplan_kernel::ids::UserId;
use serde::{Deserialize, Serialize};

/// 域错误码前缀(`PM-xxx`)。
pub const DOMAIN_CODE: &str = "PM";

/// 表前缀约定(`proj_*`,见 crates/AGENTS.md)。
pub const TABLE_PREFIX: &str = "proj_";

define_id!(
    /// 项目 ID。
    ProjectId
);
define_id!(
    /// 工作项 ID。
    WorkItemId
);
define_id!(
    /// 迭代 ID。
    SprintId
);

/// 工作项类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemType {
    /// 史诗。
    Epic,
    /// 需求。
    Story,
    /// 任务。
    Task,
    /// 缺陷。
    Bug,
}

impl ItemType {
    /// 数据库存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Epic => "epic",
            Self::Story => "story",
            Self::Task => "task",
            Self::Bug => "bug",
        }
    }

    /// 从数据库值解析。
    #[must_use]
    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value {
            "epic" => Some(Self::Epic),
            "story" => Some(Self::Story),
            "task" => Some(Self::Task),
            "bug" => Some(Self::Bug),
            _ => None,
        }
    }
}

/// 优先级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// 最高。
    Highest,
    /// 高。
    High,
    /// 中(默认)。
    Medium,
    /// 低。
    Low,
    /// 最低。
    Lowest,
}

impl Priority {
    /// 数据库存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Highest => "highest",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::Lowest => "lowest",
        }
    }

    /// 从数据库值解析(未知值回落 medium)。
    #[must_use]
    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value {
            "highest" => Some(Self::Highest),
            "high" => Some(Self::High),
            "medium" => Some(Self::Medium),
            "low" => Some(Self::Low),
            "lowest" => Some(Self::Lowest),
            _ => None,
        }
    }
}

/// 项目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// 项目 ID。
    pub id: ProjectId,
    /// 项目键(唯一,工作项编号前缀,如 `CP`)。
    pub key: String,
    /// 项目名。
    pub name: String,
    /// 描述。
    pub description: String,
    /// 负责人。
    pub lead: Option<UserId>,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 工作项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    /// 工作项 ID。
    pub id: WorkItemId,
    /// 所属项目。
    pub project_id: ProjectId,
    /// 项目内编号(与项目 key 组成展示编号,如 `CP-1`)。
    pub item_number: i64,
    /// 类型。
    pub item_type: ItemType,
    /// 标题。
    pub title: String,
    /// 描述(Markdown)。
    pub description: String,
    /// 当前状态(状态机)。
    pub status: String,
    /// 优先级。
    pub priority: Priority,
    /// 经办人。
    pub assignee: Option<UserId>,
    /// 报告人。
    pub reporter: Option<UserId>,
    /// 自定义字段(JSONB)。
    pub custom_fields: serde_json::Value,
    /// 父工作项(需求树:epic → story → task)。
    pub parent_id: Option<WorkItemId>,
    /// 看板列内排序(小在前)。
    pub board_order: i64,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 创建项目的入参。
#[derive(Debug, Clone)]
pub struct NewProject {
    /// 项目键(大写字母/数字/连字符)。
    pub key: String,
    /// 项目名。
    pub name: String,
    /// 描述。
    pub description: String,
    /// 负责人(通常为创建者)。
    pub lead: Option<UserId>,
}

/// 创建工作项的入参。
#[derive(Debug, Clone)]
pub struct NewWorkItem {
    /// 类型。
    pub item_type: ItemType,
    /// 标题。
    pub title: String,
    /// 描述。
    pub description: String,
    /// 优先级(缺省 medium)。
    pub priority: Option<Priority>,
    /// 经办人。
    pub assignee: Option<UserId>,
    /// 报告人(通常为创建者)。
    pub reporter: Option<UserId>,
    /// 自定义字段。
    pub custom_fields: serde_json::Value,
    /// 父工作项。
    pub parent_id: Option<WorkItemId>,
}

/// 项目仓储抽象。
#[async_trait::async_trait]
pub trait ProjectRepository: Send + Sync {
    /// 创建项目(key 唯一冲突返回 `PM-409`)。
    ///
    /// # Errors
    /// 存储故障或唯一键冲突。
    async fn create(&self, project: &NewProject) -> Result<Project, AppError>;

    /// 按键查询。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_key(&self, key: &str) -> Result<Option<Project>, AppError>;

    /// 全量项目列表(企业内部量级可控)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list(&self) -> Result<Vec<Project>, AppError>;
}

/// 工作项仓储抽象。
#[async_trait::async_trait]
pub trait WorkItemRepository: Send + Sync {
    /// 创建工作项(项目内编号自动分配;初始状态由引擎决定)。
    ///
    /// # Errors
    /// 存储故障。
    async fn create(
        &self,
        project_id: ProjectId,
        item: &NewWorkItem,
        initial_status: &str,
        reporter: Option<UserId>,
    ) -> Result<WorkItem, AppError>;

    /// 按 ID 查询。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_id(&self, id: WorkItemId) -> Result<Option<WorkItem>, AppError>;

    /// 按项目内编号查询(提交关联 `#KEY-N` 解析)。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_number(
        &self,
        project_id: ProjectId,
        item_number: i64,
    ) -> Result<Option<WorkItem>, AppError>;

    /// 按项目过滤列表(全空过滤 = 全量;按创建时间倒序)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list(
        &self,
        project_id: ProjectId,
        item_type: Option<ItemType>,
        status: Option<&str>,
        assignee: Option<UserId>,
    ) -> Result<Vec<WorkItem>, AppError>;

    /// 更新可编辑字段(标题/描述/优先级/经办人/自定义字段)。
    ///
    /// # Errors
    /// 不存在返回 `PM-404`;存储故障。
    async fn update_fields(
        &self,
        id: WorkItemId,
        title: Option<&str>,
        description: Option<&str>,
        priority: Option<Priority>,
        assignee: Option<Option<UserId>>,
        custom_fields: Option<&serde_json::Value>,
    ) -> Result<WorkItem, AppError>;

    /// 状态流转(调用方已通过引擎校验合法性)。
    ///
    /// # Errors
    /// 不存在返回 `PM-404`;存储故障。
    async fn update_status(&self, id: WorkItemId, status: &str) -> Result<WorkItem, AppError>;

    /// 关联/移除迭代(None 移除)。
    ///
    /// # Errors
    /// 存储故障。
    async fn set_sprint(&self, id: WorkItemId, sprint_id: Option<SprintId>)
    -> Result<(), AppError>;

    /// 关联/移除里程碑。
    ///
    /// # Errors
    /// 存储故障。
    async fn set_milestone(
        &self,
        id: WorkItemId,
        milestone_id: Option<MilestoneId>,
    ) -> Result<(), AppError>;

    /// 看板批量重排:更新状态与列内排序(拖拽)。
    ///
    /// # Errors
    /// 存储故障。
    async fn set_board_positions(
        &self,
        positions: &[(WorkItemId, String, i64)],
    ) -> Result<(), AppError>;

    /// 跨项目按人聚合(工作台):assignee 或 reporter 为该用户。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_for_user(&self, user: UserId) -> Result<Vec<WorkItem>, AppError>;
}

/// 迭代状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SprintStatus {
    /// 未开始。
    Planned,
    /// 进行中(每项目同时至多一个)。
    Active,
    /// 已完成。
    Completed,
}

impl SprintStatus {
    /// 数据库存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Active => "active",
            Self::Completed => "completed",
        }
    }

    /// 从数据库值解析。
    #[must_use]
    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value {
            "planned" => Some(Self::Planned),
            "active" => Some(Self::Active),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }
}

/// 迭代。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sprint {
    /// 迭代 ID。
    pub id: SprintId,
    /// 所属项目。
    pub project_id: ProjectId,
    /// 名称。
    pub name: String,
    /// 目标。
    pub goal: String,
    /// 开始时间。
    pub starts_at: Option<DateTime<Utc>>,
    /// 结束时间。
    pub ends_at: Option<DateTime<Utc>>,
    /// 容量(点数)。
    pub capacity_points: i32,
    /// 状态。
    pub status: SprintStatus,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 新建迭代入参。
#[derive(Debug, Clone)]
pub struct NewSprint {
    /// 名称。
    pub name: String,
    /// 目标。
    pub goal: String,
    /// 开始时间。
    pub starts_at: Option<DateTime<Utc>>,
    /// 结束时间。
    pub ends_at: Option<DateTime<Utc>>,
    /// 容量(点数)。
    pub capacity_points: i32,
}

/// 里程碑。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Milestone {
    /// 里程碑 ID。
    pub id: MilestoneId,
    /// 所属项目。
    pub project_id: ProjectId,
    /// 名称。
    pub name: String,
    /// 描述。
    pub description: String,
    /// 截止日期(UTC 午夜)。
    pub due_date: Option<DateTime<Utc>>,
    /// 状态(open/closed)。
    pub status: String,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

define_id!(
    /// 里程碑 ID。
    MilestoneId
);

/// 新建里程碑入参。
#[derive(Debug, Clone)]
pub struct NewMilestone {
    /// 名称。
    pub name: String,
    /// 描述。
    pub description: String,
    /// 截止日期。
    pub due_date: Option<DateTime<Utc>>,
}

/// 迭代进度(汇总)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SprintProgress {
    /// 迭代内工作项总数。
    pub total: i64,
    /// 已完成(status = done)数。
    pub done: i64,
    /// 已提交点数(自定义字段 story_points 求和)。
    pub committed_points: i64,
}

/// 里程碑进度。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MilestoneProgress {
    /// 关联工作项总数。
    pub total: i64,
    /// 已完成数。
    pub done: i64,
}

/// 迭代仓储抽象。
#[async_trait::async_trait]
pub trait SprintRepository: Send + Sync {
    /// 创建迭代。
    ///
    /// # Errors
    /// 存储故障。
    async fn create(&self, project_id: ProjectId, sprint: &NewSprint) -> Result<Sprint, AppError>;

    /// 项目迭代列表(创建时间倒序)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_project(&self, project_id: ProjectId) -> Result<Vec<Sprint>, AppError>;

    /// 查迭代。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_id(&self, id: SprintId) -> Result<Option<Sprint>, AppError>;

    /// 更新状态(启动/完成)。
    ///
    /// # Errors
    /// 存储故障。
    async fn update_status(&self, id: SprintId, status: SprintStatus) -> Result<(), AppError>;

    /// 将某项目全部 active 迭代降级为 planned(启动新迭代前调用)。
    ///
    /// # Errors
    /// 存储故障。
    async fn demote_active(&self, project_id: ProjectId) -> Result<(), AppError>;

    /// 迭代进度(工作项总数/完成数/提交点数)。
    ///
    /// # Errors
    /// 存储故障。
    async fn progress(&self, id: SprintId) -> Result<SprintProgress, AppError>;
}

/// 里程碑仓储抽象。
#[async_trait::async_trait]
pub trait MilestoneRepository: Send + Sync {
    /// 创建里程碑。
    ///
    /// # Errors
    /// 存储故障。
    async fn create(
        &self,
        project_id: ProjectId,
        milestone: &NewMilestone,
    ) -> Result<Milestone, AppError>;

    /// 项目里程碑列表。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_project(&self, project_id: ProjectId) -> Result<Vec<Milestone>, AppError>;

    /// 查里程碑。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_id(&self, id: MilestoneId) -> Result<Option<Milestone>, AppError>;

    /// 关闭里程碑。
    ///
    /// # Errors
    /// 存储故障。
    async fn close(&self, id: MilestoneId) -> Result<(), AppError>;

    /// 里程碑进度。
    ///
    /// # Errors
    /// 存储故障。
    async fn progress(&self, id: MilestoneId) -> Result<MilestoneProgress, AppError>;
}

/// 缺陷严重程度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Severity {
    /// 阻断。
    Blocker,
    /// 严重。
    Critical,
    /// 一般。
    Major,
    /// 轻微。
    Minor,
    /// 微小。
    Trivial,
}

impl Severity {
    /// 存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blocker => "blocker",
            Self::Critical => "critical",
            Self::Major => "major",
            Self::Minor => "minor",
            Self::Trivial => "trivial",
        }
    }

    /// 从字符串解析。
    #[must_use]
    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value {
            "blocker" => Some(Self::Blocker),
            "critical" => Some(Self::Critical),
            "major" => Some(Self::Major),
            "minor" => Some(Self::Minor),
            "trivial" => Some(Self::Trivial),
            _ => None,
        }
    }
}

/// 缺陷特化字段(T-114):存于 `custom_fields`,经类型化校验。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BugFields {
    /// 严重程度。
    pub severity: Severity,
    /// 复现步骤(流转到 resolved 前必须补齐,回归依据)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repro_steps: Option<String>,
}

impl BugFields {
    /// 缺省严重程度(未显式给出时注入)。
    pub const DEFAULT_SEVERITY: &'static str = "major";

    /// 从 custom_fields 解析;缺 severity 时注入默认值。
    ///
    /// # Errors
    /// severity 非法(不在枚举内)返回 `PM-400`。
    pub fn from_custom_fields(fields: &serde_json::Value) -> Result<Self, AppError> {
        let severity_str = fields
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or(Self::DEFAULT_SEVERITY);
        let severity = Severity::from_str_opt(severity_str).ok_or_else(|| {
            AppError::invalid(
                DOMAIN_CODE,
                format!("非法严重程度:{severity_str}(可选 blocker/critical/major/minor/trivial)"),
            )
        })?;
        Ok(Self {
            severity,
            repro_steps: fields
                .get("repro_steps")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
        })
    }

    /// 写回 custom_fields(合并而非覆盖其他自定义字段)。
    #[must_use]
    pub fn merged_into(&self, fields: &serde_json::Value) -> serde_json::Value {
        let mut out = fields.clone();
        let obj = out.as_object_mut().expect("custom_fields 应为对象");
        obj.insert(
            "severity".to_owned(),
            serde_json::Value::String(self.severity.as_str().to_owned()),
        );
        if let Some(steps) = &self.repro_steps {
            obj.insert(
                "repro_steps".to_owned(),
                serde_json::Value::String(steps.clone()),
            );
        }
        out
    }

    /// 流转到 resolved 的门禁:复现步骤必须已填写(回归依据)。
    #[must_use]
    pub fn ready_to_resolve(&self) -> bool {
        self.repro_steps
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
    }
}

#[cfg(test)]
mod bug_fields_tests {
    use super::*;

    #[test]
    fn missing_severity_defaults_to_major() {
        let f = BugFields::from_custom_fields(&serde_json::json!({})).expect("应成功");
        assert_eq!(f.severity, Severity::Major);
    }

    #[test]
    fn invalid_severity_rejected() {
        let err = BugFields::from_custom_fields(&serde_json::json!({"severity": "mega"}))
            .expect_err("非法严重程度应拒绝");
        assert_eq!(err.code(), "PM-400");
    }

    #[test]
    fn valid_severity_and_repro_roundtrip() {
        let f = BugFields::from_custom_fields(&serde_json::json!({
            "severity": "blocker", "repro_steps": "1. 打开 2. 爆炸"
        }))
        .expect("应成功");
        assert_eq!(f.severity, Severity::Blocker);
        assert!(f.ready_to_resolve());
        let merged = f.merged_into(&serde_json::json!({"custom": 1}));
        assert_eq!(merged["severity"], "blocker");
        assert_eq!(merged["custom"], 1, "不应覆盖其他自定义字段");
    }

    #[test]
    fn resolve_gate_requires_repro() {
        let no_repro = BugFields::from_custom_fields(&serde_json::json!({"severity": "minor"}))
            .expect("应成功");
        assert!(!no_repro.ready_to_resolve());
        let blank = BugFields::from_custom_fields(&serde_json::json!({
            "severity": "minor", "repro_steps": "   "
        }))
        .expect("应成功");
        assert!(!blank.ready_to_resolve(), "空白复现步骤不算已填");
    }
}

define_id!(
    /// 提交链接 ID。
    CommitLinkId
);

/// 工作项关联的提交(推送时解析 `#KEY-N` 建立,T-207)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItemCommit {
    /// 链接 ID。
    pub id: CommitLinkId,
    /// 工作项 ID。
    pub work_item_id: WorkItemId,
    /// 仓库 ID(跨域引用,不加 FK)。
    pub repo_id: uuid::Uuid,
    /// 仓库全名。
    pub full_name: String,
    /// 提交 SHA。
    pub commit_sha: String,
    /// 提交标题。
    pub subject: String,
    /// 引用全名。
    pub ref_name: String,
    /// 提交时间(可空,payload 未带时为空)。
    pub committed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// 关联时间。
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// 新提交链接入参。
#[derive(Debug, Clone)]
pub struct NewCommitLink {
    /// 工作项 ID。
    pub work_item_id: WorkItemId,
    /// 仓库 ID。
    pub repo_id: uuid::Uuid,
    /// 仓库全名。
    pub full_name: String,
    /// 提交 SHA。
    pub commit_sha: String,
    /// 提交标题。
    pub subject: String,
    /// 引用全名。
    pub ref_name: String,
}

/// 提交链接仓储。
#[async_trait::async_trait]
pub trait CommitLinkRepository: Send + Sync {
    /// 建链(重复幂等)。
    ///
    /// # Errors
    /// 存储故障。
    async fn link(&self, link: &NewCommitLink) -> Result<(), AppError>;

    /// 工作项的提交链接(时间倒序)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_item(&self, work_item_id: WorkItemId)
    -> Result<Vec<WorkItemCommit>, AppError>;
}

define_id!(
    /// 工作项评论 ID。
    CommentId
);

/// 工作项评论(T-132):详情页对话流。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItemComment {
    /// 评论 ID。
    pub id: CommentId,
    /// 工作项 ID。
    pub work_item_id: WorkItemId,
    /// 作者(跨域引用 identity,不加 FK;系统评论为空)。
    pub author_id: Option<UserId>,
    /// 正文(Markdown)。
    pub body: String,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 新增评论入参。
#[derive(Debug, Clone)]
pub struct NewComment {
    /// 工作项 ID。
    pub work_item_id: WorkItemId,
    /// 作者。
    pub author_id: Option<UserId>,
    /// 正文(去空白后不得为空)。
    pub body: String,
}

/// 评论仓储。
#[async_trait::async_trait]
pub trait CommentRepository: Send + Sync {
    /// 新增并返回落库结果。
    ///
    /// # Errors
    /// 存储故障。
    async fn add(&self, comment: &NewComment) -> Result<WorkItemComment, AppError>;

    /// 某工作项的评论(时间正序)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_item(&self, work_item_id: WorkItemId)
    -> Result<Vec<WorkItemComment>, AppError>;

    /// 按 ID 查询(删除前的权限判定用)。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_id(&self, id: CommentId) -> Result<Option<WorkItemComment>, AppError>;

    /// 删除,返回是否命中。
    ///
    /// # Errors
    /// 存储故障。
    async fn delete(&self, id: CommentId) -> Result<bool, AppError>;
}

define_id!(
    /// 工时记录 ID。
    WorklogId
);

/// 工时登记(T-318 / PM-09)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worklog {
    /// 记录 ID。
    pub id: WorklogId,
    /// 工作项 ID。
    pub work_item_id: WorkItemId,
    /// 登记人(跨域引用 identity,不加 FK)。
    pub author_id: Option<UserId>,
    /// 用时(分钟,正数)。
    pub minutes: i64,
    /// 备注。
    pub note: String,
    /// 工作日期。
    pub spent_on: chrono::NaiveDate,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 新增工时入参。
#[derive(Debug, Clone)]
pub struct NewWorklog {
    /// 工作项 ID。
    pub work_item_id: WorkItemId,
    /// 登记人。
    pub author_id: Option<UserId>,
    /// 用时(分钟)。
    pub minutes: i64,
    /// 备注。
    pub note: String,
    /// 工作日期(空 = 今天)。
    pub spent_on: Option<chrono::NaiveDate>,
}

/// 工时汇总。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WorklogSummary {
    /// 累计登记(分钟)。
    pub logged_minutes: i64,
    /// 预估(分钟;取自 `custom_fields.estimate_minutes`,无则为空)。
    pub estimate_minutes: Option<i64>,
    /// 记录数。
    pub entries: usize,
    /// 按人汇总(用户 ID 字符串 → 分钟)。
    pub by_author: std::collections::BTreeMap<String, i64>,
}

/// 工时仓储。
#[async_trait::async_trait]
pub trait WorklogRepository: Send + Sync {
    /// 新增并返回落库结果。
    ///
    /// # Errors
    /// 存储故障。
    async fn add(&self, log: &NewWorklog) -> Result<Worklog, AppError>;

    /// 某工作项的工时(日期倒序)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_item(&self, work_item_id: WorkItemId) -> Result<Vec<Worklog>, AppError>;

    /// 按 ID 查询(删除前权限判定)。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_id(&self, id: WorklogId) -> Result<Option<Worklog>, AppError>;

    /// 删除,返回是否命中。
    ///
    /// # Errors
    /// 存储故障。
    async fn delete(&self, id: WorklogId) -> Result<bool, AppError>;
}

/// 工时汇总(纯函数,便于单测)。
#[must_use]
pub fn summarize_worklogs(logs: &[Worklog], estimate_minutes: Option<i64>) -> WorklogSummary {
    let mut by_author: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
    let mut total = 0i64;
    for l in logs {
        total += l.minutes;
        let key = l
            .author_id
            .map(|u| u.as_uuid().to_string())
            .unwrap_or_else(|| "system".to_owned());
        *by_author.entry(key).or_insert(0) += l.minutes;
    }
    WorklogSummary {
        logged_minutes: total,
        estimate_minutes,
        entries: logs.len(),
        by_author,
    }
}

#[cfg(test)]
mod worklog_tests {
    use super::*;

    fn log(author: Option<UserId>, minutes: i64) -> Worklog {
        Worklog {
            id: WorklogId::from_uuid(uuid::Uuid::new_v4()),
            work_item_id: WorkItemId::from_uuid(uuid::Uuid::nil()),
            author_id: author,
            minutes,
            note: String::new(),
            spent_on: chrono::NaiveDate::from_ymd_opt(2026, 10, 12).expect("合法日期"),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn summary_totals_and_groups_by_author() {
        let a = UserId::from_uuid(uuid::Uuid::from_u128(1));
        let b = UserId::from_uuid(uuid::Uuid::from_u128(2));
        let logs = vec![
            log(Some(a), 60),
            log(Some(a), 30),
            log(Some(b), 120),
            log(None, 15),
        ];
        let s = summarize_worklogs(&logs, Some(480));
        assert_eq!(s.logged_minutes, 225);
        assert_eq!(s.entries, 4);
        assert_eq!(s.estimate_minutes, Some(480));
        assert_eq!(s.by_author.get(&a.as_uuid().to_string()), Some(&90));
        assert_eq!(s.by_author.get(&b.as_uuid().to_string()), Some(&120));
        assert_eq!(s.by_author.get("system"), Some(&15));
    }

    #[test]
    fn empty_summary_is_zero() {
        let s = summarize_worklogs(&[], None);
        assert_eq!(s.logged_minutes, 0);
        assert_eq!(s.entries, 0);
        assert!(s.by_author.is_empty());
    }
}
