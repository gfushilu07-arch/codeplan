//! Webhook 仓储 PG 实现(T-209)。

use codeplan_common::error::AppError;
use codeplan_kernel::ids::RepoId;
use sqlx::{PgPool, Row};

use crate::webhook::{
    NewWebhook, Webhook, WebhookDelivery, WebhookDeliveryId, WebhookId, WebhookRepository,
};

const DC: &str = "GIT";

fn db_err(e: impl std::fmt::Display) -> AppError {
    AppError::internal(DC).with_details(serde_json::json!(e.to_string()))
}

const HOOK_COLS: &str = "id, repo_id, url, secret, events, active, created_at, updated_at";

fn hook_row(row: sqlx::postgres::PgRow) -> Webhook {
    Webhook {
        id: WebhookId::from_uuid(row.get::<uuid::Uuid, _>("id")),
        repo_id: RepoId::from_uuid(row.get::<uuid::Uuid, _>("repo_id")),
        url: row.get("url"),
        secret: row.get("secret"),
        events: row.get("events"),
        active: row.get("active"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

/// PG 实现。
pub struct PgWebhookStore {
    pool: PgPool,
}

impl PgWebhookStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl WebhookRepository for PgWebhookStore {
    async fn create(&self, hook: &NewWebhook) -> Result<Webhook, AppError> {
        let row = sqlx::query(&format!(
            "INSERT INTO repo_webhook (id, repo_id, url, secret, events)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING {HOOK_COLS}"
        ))
        .bind(WebhookId::generate().as_uuid())
        .bind(hook.repo_id.as_uuid())
        .bind(&hook.url)
        .bind(&hook.secret)
        .bind(&hook.events)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(hook_row(row))
    }

    async fn list_by_repo(&self, repo_id: RepoId) -> Result<Vec<Webhook>, AppError> {
        let rows = sqlx::query(&format!(
            "SELECT {HOOK_COLS} FROM repo_webhook WHERE repo_id = $1 ORDER BY created_at"
        ))
        .bind(repo_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(hook_row).collect())
    }

    async fn find_by_id(&self, id: WebhookId) -> Result<Option<Webhook>, AppError> {
        let row = sqlx::query(&format!(
            "SELECT {HOOK_COLS} FROM repo_webhook WHERE id = $1"
        ))
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(hook_row))
    }

    async fn delete(&self, id: WebhookId) -> Result<(), AppError> {
        let n = sqlx::query("DELETE FROM repo_webhook WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(AppError::not_found(DC, "Webhook 不存在"));
        }
        Ok(())
    }

    async fn log_delivery(&self, d: &WebhookDelivery) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO repo_webhook_delivery
             (id, webhook_id, event_type, request_body, status_code, error, attempts, ok)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(WebhookDeliveryId::generate().as_uuid())
        .bind(d.webhook_id.as_uuid())
        .bind(&d.event_type)
        .bind(&d.request_body)
        .bind(d.status_code)
        .bind(&d.error)
        .bind(d.attempts)
        .bind(d.ok)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn list_deliveries(
        &self,
        webhook_id: WebhookId,
        limit: i64,
    ) -> Result<Vec<WebhookDelivery>, AppError> {
        let rows = sqlx::query(
            "SELECT id, webhook_id, event_type, request_body, status_code, error, attempts, ok, created_at
             FROM repo_webhook_delivery WHERE webhook_id = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(webhook_id.as_uuid())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|row| WebhookDelivery {
                id: WebhookDeliveryId::from_uuid(row.get::<uuid::Uuid, _>("id")),
                webhook_id: WebhookId::from_uuid(row.get::<uuid::Uuid, _>("webhook_id")),
                event_type: row.get("event_type"),
                request_body: row.get("request_body"),
                status_code: row.get("status_code"),
                error: row.get("error"),
                attempts: row.get("attempts"),
                ok: row.get("ok"),
                created_at: row.get("created_at"),
            })
            .collect())
    }
}
