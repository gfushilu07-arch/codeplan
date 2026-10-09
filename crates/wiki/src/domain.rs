//! Wiki 域实体与仓储 trait(纯领域,不依赖 axum/sqlx)。

use chrono::{DateTime, Utc};
use codeplan_common::define_id;
use codeplan_common::error::AppError;
use codeplan_kernel::ids::ProjectId;
use codeplan_kernel::ids::UserId;
use serde::{Deserialize, Serialize};

define_id!(
    /// Wiki 页面 ID。
    PageId
);
define_id!(
    /// 附件 ID。
    AttachmentId
);

/// Wiki 页面(文档树节点)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiPage {
    /// 页面 ID。
    pub id: PageId,
    /// 所属项目。
    pub project_id: ProjectId,
    /// URL 标识(项目内唯一,`[a-z0-9-]+`)。
    pub slug: String,
    /// 标题。
    pub title: String,
    /// Markdown 源内容。
    pub content: String,
    /// 父页面(文档树)。
    pub parent_id: Option<PageId>,
    /// 最后修改者。
    pub updated_by: Option<UserId>,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 页面摘要(树/列表用,不含正文)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageSummary {
    /// 页面 ID。
    pub id: PageId,
    /// slug。
    pub slug: String,
    /// 标题。
    pub title: String,
    /// 父页面。
    pub parent_id: Option<PageId>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 新建/更新页面入参。
#[derive(Debug, Clone)]
pub struct PageUpsert {
    /// 标题。
    pub title: String,
    /// Markdown 内容。
    pub content: String,
    /// 父页面。
    pub parent_id: Option<PageId>,
}

/// 附件元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// 附件 ID。
    pub id: AttachmentId,
    /// 所属页面。
    pub page_id: PageId,
    /// 文件名。
    pub filename: String,
    /// 字节数。
    pub size_bytes: i64,
    /// 内容类型。
    pub content_type: Option<String>,
    /// 存储键(内部)。
    pub storage_key: String,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 页面仓储抽象。
#[async_trait::async_trait]
pub trait PageRepository: Send + Sync {
    /// 创建页面(slug 项目内唯一冲突返回 `WIKI-409`)。
    ///
    /// # Errors
    /// 存储故障或唯一键冲突。
    async fn create(
        &self,
        project_id: ProjectId,
        slug: &str,
        title: &str,
        content: &str,
        parent_id: Option<PageId>,
        updated_by: Option<UserId>,
    ) -> Result<WikiPage, AppError>;

    /// 更新页面(标题/内容/父节点)。
    ///
    /// # Errors
    /// 不存在 `WIKI-404`;存储故障。
    async fn update(
        &self,
        id: PageId,
        title: &str,
        content: &str,
        parent_id: Option<PageId>,
        updated_by: Option<UserId>,
    ) -> Result<WikiPage, AppError>;

    /// 按 slug 查询。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_slug(
        &self,
        project_id: ProjectId,
        slug: &str,
    ) -> Result<Option<WikiPage>, AppError>;

    /// 按 ID 查询。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_id(&self, id: PageId) -> Result<Option<WikiPage>, AppError>;

    /// 项目页面摘要列表(树由服务层组装)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_project(&self, project_id: ProjectId) -> Result<Vec<PageSummary>, AppError>;

    /// 删除页面(附件级联删除由数据库/存储层处理)。
    ///
    /// # Errors
    /// 存储故障。
    async fn delete(&self, id: PageId) -> Result<(), AppError>;
}

/// 附件元数据仓储抽象。
#[async_trait::async_trait]
pub trait AttachmentRepository: Send + Sync {
    /// 写入附件元数据。
    ///
    /// # Errors
    /// 存储故障。
    async fn create(
        &self,
        page_id: PageId,
        filename: &str,
        size_bytes: i64,
        content_type: Option<&str>,
        storage_key: &str,
        uploaded_by: Option<UserId>,
    ) -> Result<Attachment, AppError>;

    /// 按 ID 查元数据。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_id(&self, id: AttachmentId) -> Result<Option<Attachment>, AppError>;

    /// 页面附件列表。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_page(&self, page_id: PageId) -> Result<Vec<Attachment>, AppError>;

    /// 删除元数据(返回存储 key 供调用方删文件)。
    ///
    /// # Errors
    /// 存储故障。
    async fn delete(&self, id: AttachmentId) -> Result<Option<String>, AppError>;
}
