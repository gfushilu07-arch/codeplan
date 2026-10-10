//! 通知实体与存储 trait。

use chrono::{DateTime, Utc};
use codeplan_common::error::AppError;
use codeplan_kernel::ids::UserId;

/// 站内通知。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// 通知 ID。
    pub id: uuid::Uuid,
    /// 接收人。
    pub recipient: UserId,
    /// 标题。
    pub title: String,
    /// 正文。
    pub body: String,
    /// 前端跳转路径。
    pub link: Option<String>,
    /// 渠道(如 inbox)。
    pub channel: String,
    /// 已读时间。
    pub read_at: Option<DateTime<Utc>>,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 通知草稿(事件映射产物)。
#[derive(Debug, Clone)]
pub struct NotificationDraft {
    /// 接收人。
    pub recipient: UserId,
    /// 标题。
    pub title: String,
    /// 正文。
    pub body: String,
    /// 跳转路径。
    pub link: Option<String>,
}

/// 通知存储抽象。
#[async_trait::async_trait]
pub trait NotificationStore: Send + Sync {
    /// 写入通知。
    ///
    /// # Errors
    /// 存储故障。
    async fn insert(&self, draft: &NotificationDraft, channel: &str) -> Result<(), AppError>;

    /// 接收人通知列表(时间倒序)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_for(
        &self,
        recipient: UserId,
        unread_only: bool,
        limit: i64,
    ) -> Result<Vec<Notification>, AppError>;

    /// 未读数。
    ///
    /// # Errors
    /// 存储故障。
    async fn unread_count(&self, recipient: UserId) -> Result<i64, AppError>;

    /// 标记单条已读(仅接收人本人)。
    ///
    /// # Errors
    /// 存储故障。
    async fn mark_read(&self, id: uuid::Uuid, recipient: UserId) -> Result<(), AppError>;

    /// 全部标记已读。
    ///
    /// # Errors
    /// 存储故障。
    async fn mark_all_read(&self, recipient: UserId) -> Result<(), AppError>;
}
