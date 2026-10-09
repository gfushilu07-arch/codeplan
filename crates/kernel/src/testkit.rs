//! 测试夹具:内存事件总线等(仅测试代码使用,不得进入生产路径)。

use std::sync::Mutex;

use crate::events::{EventBus, EventEnvelope};

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
