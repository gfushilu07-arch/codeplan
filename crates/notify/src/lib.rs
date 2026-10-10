//! 通知基础设施(L2):站内信存储、通知 API、领域事件 → 通知的订阅分发。
//!
//! 渠道投递经 ext::PluginRegistry(T-117),站内信渠道为首个官方内置插件。

pub mod api;
pub mod domain;
pub mod infra;
pub mod listen;

/// 域错误码前缀(`NTF-xxx`)。
pub const DOMAIN_CODE: &str = "NTF";

/// 表前缀约定(`ntf_*`)。
pub const TABLE_PREFIX: &str = "ntf_";
