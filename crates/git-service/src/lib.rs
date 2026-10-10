//! 代码域(L3):仓库生命周期、传输代理(gix 读 / git http-backend 写)、
//! 代码浏览、LFS、代码片段、讨论区(T-120 起)。
//!
//! 铁律:写路径只经 git 官方 `http-backend`/`receive-pack` 代理;读路径只经
//! `GitRepo` adapter(gix);禁止 git2/libgit2 等 C FFI。

pub mod api;
pub mod domain;
pub mod infra;
pub mod service;
pub mod storage;

/// 域错误码前缀(`GIT-xxx`)。
pub const DOMAIN_CODE: &str = "GIT";
