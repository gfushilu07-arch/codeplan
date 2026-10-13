//! 效能度量:DORA/报表/外部链接器
//!
//! 层级:L3(依赖规则见 ARCHITECTURE.md §1.1,CI 由 xtask lint-deps 强制)。

pub mod api;
pub mod dora;
pub mod dora_listen;
pub mod export;
pub mod efficiency;
pub mod linker;
pub mod reports;
pub mod stats;
