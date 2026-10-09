//! 平台契约层(L1):领域事件 schema、事件总线抽象、测试夹具。
//!
//! 全平台唯一事实来源:改这里的事件 schema = 破坏性变更,需版本化并评审(见 AGENTS.md 铁律)。

pub mod events;
pub mod testkit;
