//! 应用中心(L3):应用一等实体、环境与部署目标登记、变量注入。
//!
//! 层级:L3(依赖规则见 ARCHITECTURE.md §1.1,由 xtask lint-deps 强制)。

pub mod api;
pub mod crypto;
pub mod domain;
pub mod gateway;
pub mod infra;
pub mod service;

/// 域错误码前缀(`APP-xxx`)。
pub const DOMAIN_CODE: &str = "APP";

/// 表前缀约定(`app_*`)。
pub const TABLE_PREFIX: &str = "app_";
