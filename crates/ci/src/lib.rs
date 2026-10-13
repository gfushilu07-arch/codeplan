//! CI 域:workflow 解析/runner 调度/制品库/扫描编排
//!
//! 层级:L3(依赖规则见 ARCHITECTURE.md §1.1,CI 由 xtask lint-deps 强制)。

pub mod api;
pub mod artifact_api;
pub mod artifact_store;
pub mod engine;
pub mod executor;
pub mod runner;
pub mod scan;
pub mod scan_api;
pub mod store;
pub mod workflow;

/// 域错误码前缀(`CI-xxx`)。
pub const DOMAIN_CODE: &str = "CI";
