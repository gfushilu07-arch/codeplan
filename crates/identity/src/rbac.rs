//! RBAC 服务:kernel `Authz` trait 的落地实现 + 成员管理用例。
//!
//! 权限模型(ORG-03):平台管理员(`is_platform_admin`)全通过;
//! 普通用户按资源角色 read ⊂ write ⊂ admin 判定;System 资源仅平台管理员。

use std::sync::Arc;

use codeplan_common::error::AppError;
use codeplan_kernel::authz::{Action, Authz, ResourceRef};
use codeplan_kernel::identity::CurrentUser;
use codeplan_kernel::ids::UserId;

use crate::DOMAIN_CODE;
use crate::domain::{MemberEntry, RbacStore, ResourceRole, UserRepository};

/// RBAC 服务:同时是 [`Authz`] 的实现与成员管理用例的入口。
pub struct RbacService {
    /// 角色存储。
    pub store: Arc<dyn RbacStore>,
    /// 用户仓储(平台管理员判定 + 用户名解析)。
    users: Arc<dyn UserRepository>,
}

/// 资源定位(与 kernel::ResourceRef 对应,但用字符串 ID 便于 HTTP 边界传输)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceLocator {
    /// 平台级。
    System,
    /// 项目(按 ID)。
    Project(uuid::Uuid),
    /// 仓库(按 ID)。
    Repo(uuid::Uuid),
    /// 应用(按 ID)。
    App(uuid::Uuid),
}

impl ResourceLocator {
    /// 存储中的 kind 字符串。
    #[must_use]
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Project(_) => "project",
            Self::Repo(_) => "repo",
            Self::App(_) => "app",
        }
    }

    /// 资源 ID(System 为 None)。
    #[must_use]
    pub fn id(&self) -> Option<uuid::Uuid> {
        match self {
            Self::System => None,
            Self::Project(id) | Self::Repo(id) | Self::App(id) => Some(*id),
        }
    }

    /// 转换为 kernel 资源引用。
    #[must_use]
    pub fn to_ref(&self) -> ResourceRef {
        match self {
            Self::System => ResourceRef::System,
            Self::Project(id) => {
                ResourceRef::Project(codeplan_kernel::ids::ProjectId::from_uuid(*id))
            }
            Self::Repo(id) => ResourceRef::Repo(codeplan_kernel::ids::RepoId::from_uuid(*id)),
            Self::App(id) => ResourceRef::App(codeplan_kernel::ids::AppId::from_uuid(*id)),
        }
    }

    /// 从 kind 字符串 + ID 解析。
    ///
    /// # Errors
    /// kind 非法或 ID 非法 UUID 返回 `ORG-400`。
    pub fn parse(kind: &str, id: &str) -> Result<Self, AppError> {
        let bad = || AppError::invalid(DOMAIN_CODE, format!("非法资源定位:{kind}/{id}"));
        match kind {
            "system" => Ok(Self::System),
            "project" => Ok(Self::Project(id.parse().map_err(|_| bad())?)),
            "repo" => Ok(Self::Repo(id.parse().map_err(|_| bad())?)),
            "app" => Ok(Self::App(id.parse().map_err(|_| bad())?)),
            _ => Err(bad()),
        }
    }
}

impl RbacService {
    /// 构造。
    #[must_use]
    pub fn new(store: Arc<dyn RbacStore>, users: Arc<dyn UserRepository>) -> Self {
        Self { store, users }
    }

    /// 校验用户对资源具备指定动作权限,不具备时返回 `ORG-403`。
    ///
    /// # Errors
    /// 无权限返回 `ORG-403`;后端故障透传。
    pub async fn ensure(
        &self,
        user: UserId,
        action: Action,
        resource: &ResourceLocator,
    ) -> Result<(), AppError> {
        if self
            .check(user, action, &resource.to_ref())
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e)))?
        {
            return Ok(());
        }
        Err(AppError::forbidden(DOMAIN_CODE, "无权限执行该操作"))
    }

    /// 管理员授予/更新成员角色。
    ///
    /// # Errors
    /// 操作者无 admin 权限返回 `ORG-403`;用户不存在返回 `ORG-404`。
    pub async fn grant(
        &self,
        actor: &CurrentUser,
        resource: ResourceLocator,
        target_username: &str,
        role: ResourceRole,
    ) -> Result<(), AppError> {
        self.ensure_manager(actor, &resource).await?;
        let target = self
            .users
            .find_by_username(target_username)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "目标用户不存在"))?;
        let Some(id) = resource.id() else {
            return Err(AppError::invalid(DOMAIN_CODE, "系统资源不支持成员管理"));
        };
        self.store
            .grant(target.id, resource.kind_str(), id, role)
            .await
    }

    /// 管理员撤销成员角色。
    ///
    /// # Errors
    /// 操作者无 admin 权限返回 `ORG-403`。
    pub async fn revoke(
        &self,
        actor: &CurrentUser,
        resource: ResourceLocator,
        target_username: &str,
    ) -> Result<(), AppError> {
        self.ensure_manager(actor, &resource).await?;
        let Some(id) = resource.id() else {
            return Err(AppError::invalid(DOMAIN_CODE, "系统资源不支持成员管理"));
        };
        let target = self
            .users
            .find_by_username(target_username)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "目标用户不存在"))?;
        self.store.revoke(target.id, resource.kind_str(), id).await
    }

    /// 成员列表(需要资源读权限或平台管理员)。
    ///
    /// # Errors
    /// 无读权限返回 `ORG-403`。
    pub async fn members(
        &self,
        actor: &CurrentUser,
        resource: ResourceLocator,
    ) -> Result<Vec<MemberEntry>, AppError> {
        self.ensure(actor.id, Action::Read, &resource).await?;
        let Some(id) = resource.id() else {
            return Err(AppError::invalid(DOMAIN_CODE, "系统资源不支持成员管理"));
        };
        self.store.members(resource.kind_str(), id).await
    }

    /// 查询自己在资源上的角色(System 返回管理员与否)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn my_role(
        &self,
        actor: &CurrentUser,
        resource: ResourceLocator,
    ) -> Result<Option<ResourceRole>, AppError> {
        if resource == ResourceLocator::System {
            return Ok(actor.is_platform_admin.then_some(ResourceRole::Admin));
        }
        let Some(id) = resource.id() else {
            return Ok(None);
        };
        self.store.role_of(actor.id, resource.kind_str(), id).await
    }

    async fn ensure_manager(
        &self,
        actor: &CurrentUser,
        resource: &ResourceLocator,
    ) -> Result<(), AppError> {
        self.ensure(actor.id, Action::Admin, resource).await
    }
}

#[async_trait::async_trait]
impl Authz for RbacService {
    async fn check(
        &self,
        user: UserId,
        action: Action,
        resource: &ResourceRef,
    ) -> Result<bool, String> {
        // 平台管理员全通过
        let u = self
            .users
            .find_by_id(user)
            .await
            .map_err(|e| e.message().to_owned())?;
        let Some(u) = u else {
            return Ok(false);
        };
        if u.is_platform_admin {
            return Ok(true);
        }
        if matches!(resource, ResourceRef::System) {
            return Ok(false);
        }
        let (kind, rid) = match resource {
            ResourceRef::Project(id) => ("project", id.as_uuid()),
            ResourceRef::Repo(id) => ("repo", id.as_uuid()),
            ResourceRef::App(id) => ("app", id.as_uuid()),
            ResourceRef::System => unreachable!("上一分支已返回"),
        };
        let role = self
            .store
            .role_of(user, kind, rid)
            .await
            .map_err(|e| e.message().to_owned())?;
        Ok(role.is_some_and(|r| r.implies(action)))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use codeplan_kernel::authz::{Action, Authz, ResourceRef};
    use codeplan_kernel::ids::UserId;

    use crate::domain::ResourceRole;
    use crate::rbac::{RbacService, ResourceLocator};
    use crate::test_support::{InMemoryRbacStore, InMemoryUserRepository};

    async fn setup() -> (RbacService, UserId, UserId, UserId, uuid::Uuid) {
        let users = Arc::new(InMemoryUserRepository::new());
        let alice = users.seed("alice", Some("pw"), false, false).await;
        let bob = users.seed("bob", Some("pw"), false, false).await;
        let root = users.seed("root", Some("pw"), true, false).await;
        let svc = RbacService::new(
            Arc::new(InMemoryRbacStore::new(Arc::clone(&users))),
            Arc::clone(&users) as Arc<dyn UserRepositoryTrait>,
        );
        (svc, alice.id, bob.id, root.id, rid_of(root.id))
    }

    fn rid_of(_: UserId) -> uuid::Uuid {
        uuid::Uuid::new_v4()
    }

    use crate::domain::UserRepository as UserRepositoryTrait;

    #[tokio::test]
    async fn no_role_means_deny() {
        let (svc, alice, _bob, _root, rid) = setup().await;
        let proj = ResourceRef::Project(codeplan_kernel::ids::ProjectId::from_uuid(rid));
        assert!(!svc.check(alice, Action::Read, &proj).await.expect("ok"));
    }

    #[tokio::test]
    async fn role_hierarchy() {
        let (svc, alice, _bob, _root, rid) = setup().await;
        let proj = ResourceRef::Project(codeplan_kernel::ids::ProjectId::from_uuid(rid));
        svc.store
            .grant(alice, "project", rid, ResourceRole::Read)
            .await
            .expect("ok");
        assert!(svc.check(alice, Action::Read, &proj).await.expect("ok"));
        assert!(!svc.check(alice, Action::Write, &proj).await.expect("ok"));
        svc.store
            .grant(alice, "project", rid, ResourceRole::Write)
            .await
            .expect("ok");
        assert!(svc.check(alice, Action::Write, &proj).await.expect("ok"));
        assert!(!svc.check(alice, Action::Admin, &proj).await.expect("ok"));
        svc.store
            .grant(alice, "project", rid, ResourceRole::Admin)
            .await
            .expect("ok");
        assert!(svc.check(alice, Action::Admin, &proj).await.expect("ok"));
    }

    #[tokio::test]
    async fn platform_admin_bypasses() {
        let (svc, _alice, _bob, root, _rid) = setup().await;
        assert!(
            svc.check(root, Action::Admin, &ResourceRef::System)
                .await
                .expect("ok")
        );
    }

    #[tokio::test]
    async fn grant_revoke_members_flow() {
        let (svc, alice, _bob, _root, rid) = setup().await;
        svc.store
            .grant(alice, "repo", rid, ResourceRole::Write)
            .await
            .expect("ok");
        let members = svc.store.members("repo", rid).await.expect("ok");
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].username, "alice");
        assert_eq!(members[0].role, ResourceRole::Write);
        svc.store.revoke(alice, "repo", rid).await.expect("ok");
        assert!(svc.store.members("repo", rid).await.expect("ok").is_empty());
    }

    #[tokio::test]
    async fn locator_parse_and_role_of() {
        let (svc, alice, _bob, _root, rid) = setup().await;
        let loc = ResourceLocator::parse("project", &rid.to_string()).expect("合法");
        assert_eq!(loc.kind_str(), "project");
        assert!(ResourceLocator::parse("nope", &rid.to_string()).is_err());
        assert!(
            svc.my_role(
                &codeplan_kernel::identity::CurrentUser::new(alice, "a", "a@x"),
                loc
            )
            .await
            .expect("ok")
            .is_none()
        );
    }
}
