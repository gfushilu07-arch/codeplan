//! 身份与权限域(L3):用户/组织/团队、本地账号认证、JWT 会话、RBAC、审计落盘。
//!
//! 分层:`domain`(实体与仓储 trait)/ `infra`(SQLx 实现)/ `service`(用例)/
//! `api`(路由 + DTO + 认证中间件)。

pub mod api;
pub mod domain;
pub mod infra;
pub mod service;

#[cfg(test)]
pub(crate) mod test_support;

/// 域错误码前缀(错误码格式见 common::error)。
pub const DOMAIN_CODE: &str = "ORG";

/// 组织/用户相关表前缀(`org_*`,所有权见 crates/AGENTS.md)。
pub const TABLE_PREFIX: &str = "org_";
