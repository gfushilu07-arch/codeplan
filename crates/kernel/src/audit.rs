//! 审计抽象:全量操作审计经此落盘(实现见 identity 域,ORG-05)。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::authz::ResourceRef;
use crate::ids::UserId;

/// 一条审计记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// 记录 ID。
    pub id: Uuid,
    /// 操作者(系统操作可为空)。
    pub actor: Option<UserId>,
    /// 动作(如 `auth.login`、`repo.created`)。
    pub action: String,
    /// 目标资源。
    pub resource: ResourceRef,
    /// 附加细节(IP/变更前后值等,脱敏后)。
    pub detail: serde_json::Value,
    /// 发生时间(UTC)。
    pub at: DateTime<Utc>,
}

impl AuditEvent {
    /// 构造审计记录(自动生成 ID 与时间戳)。
    #[must_use]
    pub fn new(
        actor: Option<UserId>,
        action: impl Into<String>,
        resource: ResourceRef,
        detail: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            actor,
            action: action.into(),
            resource,
            detail,
            at: Utc::now(),
        }
    }
}

/// 审计落盘抽象。
#[async_trait::async_trait]
pub trait AuditSink: Send + Sync {
    /// 记录一条审计事件。审计失败不得阻断业务(实现方自行降级)。
    ///
    /// # Errors
    /// 落盘失败时返回错误,由调用方决定降级策略。
    async fn record(&self, event: AuditEvent) -> Result<(), String>;
}
