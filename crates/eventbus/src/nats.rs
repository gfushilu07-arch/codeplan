//! NATS 实现:发件箱事件 → JetStream 主题 `codeplan.events.<type>`。

use async_trait::async_trait;
use codeplan_kernel::events::{EventBus, EventEnvelope};

/// 基于 outbox 的事件总线:publish 即追加发件箱(与业务写入同事务语义由调用方保证)。
#[derive(Clone)]
pub struct OutboxEventBus {
    store: crate::outbox::PgOutboxStore,
}

impl OutboxEventBus {
    /// 构造。
    #[must_use]
    pub fn new(store: crate::outbox::PgOutboxStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl EventBus for OutboxEventBus {
    async fn publish(&self, events: Vec<EventEnvelope>) -> Result<(), String> {
        self.store
            .append(&events)
            .await
            .map_err(|e| format!("outbox 追加失败: {e}"))
    }
}

/// 事件主题名:NATS 主题点分与事件类型天然对应。
#[must_use]
pub fn subject_for(event_type: &str) -> String {
    format!("codeplan.events.{event_type}")
}
