//! 权限检查抽象:所有资源访问经此 trait(实现见 identity 域 RBAC,T-104)。

use crate::ids::{AppId, ProjectId, RepoId, UserId};

/// 资源种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    /// 平台级(系统设置/用户管理)。
    System,
    /// 项目。
    Project,
    /// 代码仓库。
    Repo,
    /// 应用。
    App,
}

/// 权限动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 读取。
    Read,
    /// 写入(工作项编辑/推送代码等)。
    Write,
    /// 管理(成员/设置/删除)。
    Admin,
}

/// 权限检查的资源引用。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ResourceRef {
    /// 平台级资源。
    System,
    /// 项目资源。
    Project(ProjectId),
    /// 仓库资源。
    Repo(RepoId),
    /// 应用资源。
    App(AppId),
}

impl ResourceRef {
    /// 资源种类。
    #[must_use]
    pub fn kind(&self) -> ResourceKind {
        match self {
            Self::System => ResourceKind::System,
            Self::Project(_) => ResourceKind::Project,
            Self::Repo(_) => ResourceKind::Repo,
            Self::App(_) => ResourceKind::App,
        }
    }
}

/// 权限检查器抽象。域代码一律经此检查,不得自行读角色表。
#[async_trait::async_trait]
pub trait Authz: Send + Sync {
    /// 判断用户对资源是否具备指定动作权限。实现方负责审计埋点之外的事,本 trait 只做判定。
    ///
    /// # Errors
    /// 权限系统自身故障(如后端存储不可用)时返回错误;无权限返回 `Ok(false)`。
    async fn check(
        &self,
        user: UserId,
        action: Action,
        resource: &ResourceRef,
    ) -> Result<bool, String>;
}
