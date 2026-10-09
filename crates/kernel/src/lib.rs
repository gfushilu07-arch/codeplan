//! 平台契约层(L1):强类型基础 ID、用户上下文、权限/审计/查询抽象、
//! 领域事件 schema、事件总线抽象、测试夹具。
//!
//! 全平台唯一事实来源:改这里 = 破坏性变更,需版本化并评审(见 AGENTS.md 铁律)。

pub mod audit;
pub mod authz;
pub mod events;
pub mod identity;
pub mod ids;
pub mod lookup;
pub mod testkit;

#[cfg(test)]
mod tests {
    use crate::events::{EventBus, EventEnvelope};
    use crate::ids::UserId;
    use crate::testkit::InMemoryEventBus;
    use serde_json::json;

    #[tokio::test]
    async fn in_memory_bus_collects_events() {
        let bus = InMemoryEventBus::new();
        let actor = UserId::generate();
        bus.publish(vec![
            EventEnvelope::new("workitem.created", json!({"id": "w1"})),
            EventEnvelope::new("pr.merged", json!({"id": "p1"}))
                .with_actor(actor.as_uuid().to_string()),
        ])
        .await
        .expect("内存总线不应失败");

        let published = bus.published();
        assert_eq!(published.len(), 2);
        assert_eq!(published[0].r#type, "workitem.created");
        assert_eq!(
            published[1].actor.as_deref(),
            Some(actor.as_uuid().to_string().as_str())
        );
    }
}
