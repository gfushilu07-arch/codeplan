//! 事件基础设施(L2):事务发件箱存储、outbox→NATS 中继、NATS 事件总线实现。
//!
//! 领域代码只依赖 kernel::EventBus 抽象(实现为本 crate 的 `OutboxEventBus`,
//! 与业务写入同事务追加);中继器保证至少一次投递,消费方必须幂等。

pub mod dispatcher;
pub mod nats;
pub mod outbox;
