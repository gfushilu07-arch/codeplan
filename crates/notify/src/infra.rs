//! 通知存储的 PostgreSQL 实现(表前缀 `ntf_*`)。

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use codeplan_common::error::AppError;
use codeplan_kernel::ids::UserId;
use sqlx::PgPool;

use crate::DOMAIN_CODE;
use crate::domain::{Notification, NotificationDraft, NotificationStore};

/// 通知存储 PG 实现。
#[derive(Clone)]
pub struct PgNotificationStore {
    pool: PgPool,
}

impl PgNotificationStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// 行映射。
#[derive(sqlx::FromRow)]
struct NotificationRow {
    id: uuid::Uuid,
    recipient: uuid::Uuid,
    title: String,
    body: String,
    link: Option<String>,
    channel: String,
    read_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl TryFrom<NotificationRow> for Notification {
    type Error = AppError;

    fn try_from(r: NotificationRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: r.id,
            recipient: UserId::from_uuid(r.recipient),
            title: r.title,
            body: r.body,
            link: r.link,
            channel: r.channel,
            read_at: r.read_at,
            created_at: r.created_at,
        })
    }
}

#[async_trait]
impl NotificationStore for PgNotificationStore {
    async fn insert(&self, draft: &NotificationDraft, channel: &str) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO ntf_notification (id, recipient, title, body, link, channel) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(draft.recipient.as_uuid())
        .bind(&draft.title)
        .bind(&draft.body)
        .bind(&draft.link)
        .bind(channel)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
        })?;
        Ok(())
    }

    async fn list_for(
        &self,
        recipient: UserId,
        unread_only: bool,
        limit: i64,
    ) -> Result<Vec<Notification>, AppError> {
        let rows = sqlx::query_as::<_, NotificationRow>(
            "SELECT id, recipient, title, body, link, channel, read_at, created_at \
             FROM ntf_notification WHERE recipient = $1 \
             AND ($2 = false OR read_at IS NULL) \
             ORDER BY created_at DESC LIMIT $3",
        )
        .bind(recipient.as_uuid())
        .bind(unread_only)
        .bind(limit.clamp(1, 200))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
        })?;
        rows.into_iter().map(Notification::try_from).collect()
    }

    async fn unread_count(&self, recipient: UserId) -> Result<i64, AppError> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM ntf_notification WHERE recipient = $1 AND read_at IS NULL",
        )
        .bind(recipient.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
        })?;
        Ok(count)
    }

    async fn mark_read(&self, id: uuid::Uuid, recipient: UserId) -> Result<(), AppError> {
        sqlx::query("UPDATE ntf_notification SET read_at = now() WHERE id = $1 AND recipient = $2")
            .bind(id)
            .bind(recipient.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| {
                AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
            })?;
        Ok(())
    }

    async fn mark_all_read(&self, recipient: UserId) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE ntf_notification SET read_at = now() WHERE recipient = $1 AND read_at IS NULL",
        )
        .bind(recipient.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
        })?;
        Ok(())
    }
}
