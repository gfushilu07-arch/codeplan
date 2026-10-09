//! 项目管理域实体与仓储 trait(纯领域,不依赖 axum/sqlx)。

use chrono::{DateTime, Utc};
use codeplan_common::define_id;
use codeplan_common::error::AppError;
use codeplan_kernel::ids::UserId;

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
}
