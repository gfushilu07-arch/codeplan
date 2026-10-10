//! 插件 SDK(L2):扩展点 trait 契约、插件注册与生命周期。
//!
//! V1 为编译期内置插件(随平台发行);V2 预留 wasmtime 动态插件。
//! 扩展点:通知渠道([`NotificationChannel`])、部署钩子、CI step、事件订阅、
//! 权限点、前端扩展点(后四者为 trait 占位,随对应域任务充实)。

pub mod channel;

pub use channel::{NotificationChannel, PluginRegistry};
