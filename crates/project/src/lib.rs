//! 项目管理域(L3):项目、工作项、状态机、迭代(T-111/T-112)。
//!
//! 分层:`domain` / `infra` / `service` / `api` / `workflow`。
//! 跨域:认证经 kernel::TokenAuthenticator 注入;权限经 kernel::Authz;
//! `project.created` 事件由 identity 监听并自动授权创建者。

pub mod api;
pub mod domain;
pub mod infra;
pub mod listen;
pub mod service;
pub mod workflow;

/// 域错误码前缀(`PM-xxx`)。
pub const DOMAIN_CODE: &str = "PM";
