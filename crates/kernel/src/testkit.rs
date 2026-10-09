//! 测试夹具:内存事件总线等(仅测试代码使用,不得进入生产路径)。

use std::sync::Mutex;

use crate::audit::{AuditEvent, AuditSink};
use crate::events::{EventBus, EventEnvelope};

/// 内存审计槽:收集审计事件供测试断言。
#[derive(Default)]
pub struct InMemoryAuditSink {
    records: Mutex<Vec<AuditEvent>>,
}

impl InMemoryAuditSink {
    /// 创建空审计槽。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 已记录审计事件快照。
    #[must_use]
    pub fn records(&self) -> Vec<AuditEvent> {
        self.records.lock().expect("审计锁中毒").clone()
    }
}

#[async_trait::async_trait]
impl AuditSink for InMemoryAuditSink {
    async fn record(&self, event: AuditEvent) -> Result<(), String> {
        self.records.lock().expect("审计锁中毒").push(event);
        Ok(())
    }
}

/// 内存事件总线:收集所有已发布事件,供测试断言。
#[derive(Default)]
pub struct InMemoryEventBus {
    published: Mutex<Vec<EventEnvelope>>,
}

impl InMemoryEventBus {
    /// 创建空总线。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 已发布事件的快照(按发布顺序)。
    #[must_use]
    pub fn published(&self) -> Vec<EventEnvelope> {
        self.published.lock().expect("事件锁中毒").clone()
    }
}

#[async_trait::async_trait]
impl EventBus for InMemoryEventBus {
    async fn publish(&self, events: Vec<EventEnvelope>) -> Result<(), String> {
        self.published.lock().expect("事件锁中毒").extend(events);
        Ok(())
    }
}
