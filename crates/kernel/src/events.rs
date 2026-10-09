//! 领域事件契约:事件信封 + 事件总线抽象(单一事实来源)。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 事件信封:所有领域事件的统一外壳。
///
/// - `r#type` 形如 `pr.merged`(域.动作过去式);
/// - `payload` 为事件类型对应的结构化数据(消费方按 type 反序列化);
/// - 发布必须随业务写入走 outbox(见 eventbus),禁止绕过。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// 全局唯一事件 ID。
    pub id: Uuid,
    /// 事件类型:`<域>.<动作过去式>`。
    pub r#type: String,
    /// 发生时间(UTC)。
    pub occurred_at: DateTime<Utc>,
    /// 触发者(用户 ID 或 system),可空。
    pub actor: Option<String>,
    /// 事件载荷。
    pub payload: serde_json::Value,
}

impl EventEnvelope {
    /// 创建事件信封(自动生成 ID 与时间戳)。
    pub fn new(event_type: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            r#type: event_type.into(),
            occurred_at: Utc::now(),
            actor: None,
            payload,
        }
    }

    /// 附带触发者。
    #[must_use]
    pub fn with_actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = Some(actor.into());
        self
    }
}

/// 事件总线抽象:域只经此发布事件(生产实现在 eventbus,测试实现在 [`crate::testkit`])。
#[async_trait::async_trait]
pub trait EventBus: Send + Sync {
    /// 发布一批事件。生产实现必须保证至少一次投递;消费方必须幂等。
    async fn publish(&self, events: Vec<EventEnvelope>) -> Result<(), String>;
}
