//! 代码域(L3):仓库生命周期、传输代理(gix 读 / git http-backend 写)、
//! 代码浏览、LFS、代码片段、讨论区(T-120 起)。
//!
//! 铁律:写路径只经 git 官方 `http-backend`/`receive-pack` 代理;读路径只经
//! `GitRepo` adapter(gix);禁止 git2/libgit2 等 C FFI。

pub mod adapter;
pub mod api;
pub mod browse;
pub mod browse_api;
pub mod domain;
pub mod infra;
pub mod lfs;
pub mod release;
pub mod release_api;
pub mod release_infra;
pub mod release_service;
pub mod service;
pub mod signing;
pub mod signing_api;
pub mod signing_gpg;
pub mod signing_ssh;
pub mod ssh;
pub mod storage;
pub mod transport;
pub mod webhook;
pub mod webhook_api;
pub mod webhook_infra;
pub mod webhook_service;

/// 域错误码前缀(`GIT-xxx`)。
pub const DOMAIN_CODE: &str = "GIT";
