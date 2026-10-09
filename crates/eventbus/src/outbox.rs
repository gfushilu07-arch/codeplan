//! 事务发件箱存储(PG)。

use codeplan_kernel::events::EventEnvelope;
use sqlx::PgPool;

use chrono::{DateTime, Utc};

/// 发件箱行(中继用)。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OutboxRow {
    /// 事件 ID。
    pub id: uuid::Uuid,
    /// 事件类型。
    pub event_type: String,
    /// 载荷。
    pub payload: serde_json::Value,
    /// 操作者。
    pub actor: Option<String>,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 发件箱存储。
#[derive(Clone)]
pub struct PgOutboxStore {
    pool: PgPool,
}

impl PgOutboxStore {
    /// 构造(共享连接池)。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 追加一批事件(生产环境应与业务写入共用同一事务——由调用方传入事务)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn append(&self, events: &[EventEnvelope]) -> Result<(), sqlx::Error> {
        for e in events {
            sqlx::query(
                "INSERT INTO event_outbox (id, event_type, payload, actor) VALUES ($1, $2, $3, $4)",
            )
            .bind(e.id)
            .bind(&e.r#type)
            .bind(&e.payload)
            .bind(&e.actor)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    /// 取未发布事件(按时间序,限量)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn fetch_unpublished(&self, limit: i64) -> Result<Vec<OutboxRow>, sqlx::Error> {
        sqlx::query_as::<_, OutboxRow>(
            "SELECT id, event_type, payload, actor, created_at FROM event_outbox \
             WHERE published_at IS NULL ORDER BY created_at LIMIT $1",
        )
        .bind(limit.clamp(1, 1000))
        .fetch_all(&self.pool)
        .await
    }

    /// 标记已发布。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn mark_published(&self, ids: &[uuid::Uuid]) -> Result<(), sqlx::Error> {
        if ids.is_empty() {
            return Ok(());
        }
        sqlx::query("UPDATE event_outbox SET published_at = now() WHERE id = ANY($1)")
            .bind(ids)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
