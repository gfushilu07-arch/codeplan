//! Webhook 出站实体与仓储 trait(T-209):订阅、投递日志。

use chrono::{DateTime, Utc};
use codeplan_common::{define_id, error::AppError};
use codeplan_kernel::ids::RepoId;

define_id!(
    /// Webhook ID。
    WebhookId
);
define_id!(
    /// 投递记录 ID。
    WebhookDeliveryId
);

/// Webhook 订阅(仓库级,事件类型空 = 全部)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Webhook {
    /// Webhook ID。
    pub id: WebhookId,
    /// 仓库 ID。
    pub repo_id: RepoId,
    /// 回调 URL。
    pub url: String,
    /// HMAC-SHA256 密钥。
    pub secret: String,
    /// 订阅的事件类型(空 = 全部)。
    pub events: Vec<String>,
    /// 是否启用。
    pub active: bool,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 投递记录(终态一条)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookDelivery {
    /// 记录 ID。
    pub id: WebhookDeliveryId,
    /// Webhook ID。
    pub webhook_id: WebhookId,
    /// 事件类型。
    pub event_type: String,
    /// 请求体。
    pub request_body: String,
    /// 最终 HTTP 状态码(网络错误为空)。
    pub status_code: Option<i32>,
    /// 错误信息。
    pub error: Option<String>,
    /// 尝试次数。
    pub attempts: i32,
    /// 是否成功。
    pub ok: bool,
    /// 时间。
    pub created_at: DateTime<Utc>,
}

/// 新建订阅入参。
#[derive(Debug, Clone)]
pub struct NewWebhook {
    /// 仓库 ID。
    pub repo_id: RepoId,
    /// 回调 URL。
    pub url: String,
    /// 密钥。
    pub secret: String,
    /// 订阅事件类型。
    pub events: Vec<String>,
}

/// Webhook 仓储。
#[async_trait::async_trait]
pub trait WebhookRepository: Send + Sync {
    /// 创建订阅。
    ///
    /// # Errors
    /// 存储故障。
    async fn create(&self, hook: &NewWebhook) -> Result<Webhook, AppError>;

    /// 仓库订阅列表。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_by_repo(&self, repo_id: RepoId) -> Result<Vec<Webhook>, AppError>;

    /// 按 ID 取。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_id(&self, id: WebhookId) -> Result<Option<Webhook>, AppError>;

    /// 删除订阅。
    ///
    /// # Errors
    /// 存储故障。
    async fn delete(&self, id: WebhookId) -> Result<(), AppError>;

    /// 追加投递日志。
    ///
    /// # Errors
    /// 存储故障。
    async fn log_delivery(&self, delivery: &WebhookDelivery) -> Result<(), AppError>;

    /// 最近投递日志(时间倒序)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_deliveries(
        &self,
        webhook_id: WebhookId,
        limit: i64,
    ) -> Result<Vec<WebhookDelivery>, AppError>;
}
