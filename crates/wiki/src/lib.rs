//! Wiki 域(L3):文档树、Markdown 渲染(comrak)、附件(fs 存储层,后续切 OpenDAL)。

pub mod api;
pub mod domain;
pub mod infra;
pub mod service;
pub mod storage;

/// 域错误码前缀(`WIKI-xxx`)。
pub const DOMAIN_CODE: &str = "WIKI";
