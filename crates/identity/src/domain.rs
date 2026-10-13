//! 身份域实体与仓储 trait(纯领域,不依赖 axum/sqlx)。

use chrono::{DateTime, Utc};
use codeplan_common::define_id;
use codeplan_common::error::AppError;
use codeplan_kernel::ids::UserId;

define_id!(
    /// 组织 ID(identity 域内别名,保持与 kernel::OrgId 一致的语义)。
    OrgId
);

impl From<OrgId> for codeplan_kernel::ids::OrgId {
    fn from(v: OrgId) -> Self {
        Self::from_uuid(v.as_uuid())
    }
}

define_id!(
    /// 团队 ID(组织下的团队分组,ORG-01)。
    TeamId
);

/// 用户来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserSource {
    /// 本地账号。
    Local,
    /// LDAP/AD 同步。
    Ldap,
    /// OIDC JIT 开通。
    Oidc,
}

impl UserSource {
    /// 数据库存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Ldap => "ldap",
            Self::Oidc => "oidc",
        }
    }

    /// 从数据库值解析。
    #[must_use]
    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value {
            "local" => Some(Self::Local),
            "ldap" => Some(Self::Ldap),
            "oidc" => Some(Self::Oidc),
            _ => None,
        }
    }
}

/// 用户状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserStatus {
    /// 正常。
    Active,
    /// 已禁用(禁止登录)。
    Disabled,
}

impl UserStatus {
    /// 数据库存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }

    /// 从数据库值解析。
    #[must_use]
    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

/// 平台用户。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// 用户 ID。
    pub id: UserId,
    /// 登录名(唯一)。
    pub username: String,
    /// 邮箱(唯一)。
    pub email: String,
    /// 显示名。
    pub display_name: String,
    /// 状态。
    pub status: UserStatus,
    /// 本地账号密码哈希(argon2;LDAP 用户为空)。
    pub password_hash: Option<String>,
    /// 用户来源。
    pub source: UserSource,
    /// TOTP 密钥(base32;setup 后 enable 前为待确认状态)。
    pub totp_secret: Option<String>,
    /// TOTP 是否已启用。
    pub totp_enabled: bool,
    /// 是否平台管理员。
    pub is_platform_admin: bool,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 创建用户的入参(密码为明文,由仓储实现负责哈希)。
#[derive(Debug, Clone)]
pub struct NewUser {
    /// 登录名。
    pub username: String,
    /// 邮箱。
    pub email: String,
    /// 显示名。
    pub display_name: String,
    /// 明文密码(可空:LDAP 同步用户先无本地密码)。
    pub password: Option<String>,
    /// 是否平台管理员。
    pub is_platform_admin: bool,
    /// 来源(local/ldap/oidc)。
    pub source: UserSource,
}

/// 用户仓储抽象。
#[async_trait::async_trait]
pub trait UserRepository: Send + Sync {
    /// 创建用户(用户名/邮箱唯一冲突应返回 `ORG-409`)。
    ///
    /// # Errors
    /// 存储故障或唯一键冲突。
    async fn create(&self, user: &NewUser) -> Result<User, codeplan_common::error::AppError>;

    /// 按登录名查询。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_username(
        &self,
        username: &str,
    ) -> Result<Option<User>, codeplan_common::error::AppError>;

    /// 按 ID 查询。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_id(
        &self,
        id: UserId,
    ) -> Result<Option<User>, codeplan_common::error::AppError>;

    /// 用户总数(用于引导管理员判断)。
    ///
    /// # Errors
    /// 存储故障。
    async fn count(&self) -> Result<i64, codeplan_common::error::AppError>;

    /// 全量用户列表(平台管理,ORG-01)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list(&self) -> Result<Vec<User>, codeplan_common::error::AppError>;

    /// 启用/禁用用户。
    ///
    /// # Errors
    /// 存储故障。
    async fn set_status(
        &self,
        id: UserId,
        status: UserStatus,
    ) -> Result<(), codeplan_common::error::AppError>;

    /// 按邮箱查询(OIDC JIT 开通用)。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_by_email(
        &self,
        email: &str,
    ) -> Result<Option<User>, codeplan_common::error::AppError>;

    /// 保存用户(TOTP 开关/密钥等状态更新)。
    ///
    /// # Errors
    /// 存储故障。
    async fn save_totp(
        &self,
        id: UserId,
        secret: Option<String>,
        enabled: bool,
    ) -> Result<(), codeplan_common::error::AppError>;

    /// LDAP 同步 upsert:按用户名插入或更新邮箱/显示名(source=ldap,无本地密码)。
    ///
    /// # Errors
    /// 存储故障。
    async fn upsert_ldap_user(
        &self,
        username: &str,
        email: &str,
        display_name: &str,
    ) -> Result<(), codeplan_common::error::AppError>;
}

/// 组织。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Organization {
    /// 组织 ID。
    pub id: OrgId,
    /// 组织名(唯一)。
    pub name: String,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 团队(组织下的分组,ORG-01)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Team {
    /// 团队 ID。
    pub id: TeamId,
    /// 所属组织。
    pub org_id: OrgId,
    /// 团队名(组织内唯一)。
    pub name: String,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 团队成员。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamMember {
    /// 团队 ID。
    pub team_id: TeamId,
    /// 成员用户 ID。
    pub user_id: UserId,
    /// 加入时间。
    pub joined_at: DateTime<Utc>,
}

/// 组织仓储抽象(组织 + 团队 + 成员)。
#[async_trait::async_trait]
pub trait OrgRepository: Send + Sync {
    /// 创建组织(名称唯一冲突应返回 `ORG-409`)。
    ///
    /// # Errors
    /// 存储故障或唯一键冲突。
    async fn create(&self, name: &str) -> Result<Organization, AppError>;

    /// 全量组织列表(企业内部量级可控)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list(&self) -> Result<Vec<Organization>, AppError>;

    /// 按 ID 取组织。
    ///
    /// # Errors
    /// 存储故障。
    async fn find(&self, id: OrgId) -> Result<Option<Organization>, AppError>;

    /// 在组织下建团队(组织内同名冲突应返回 `ORG-409`)。
    ///
    /// # Errors
    /// 存储故障或唯一键冲突。
    async fn create_team(&self, org_id: OrgId, name: &str) -> Result<Team, AppError>;

    /// 组织的团队列表。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_teams(&self, org_id: OrgId) -> Result<Vec<Team>, AppError>;

    /// 按 ID 取团队。
    ///
    /// # Errors
    /// 存储故障。
    async fn find_team(&self, id: TeamId) -> Result<Option<Team>, AppError>;

    /// 加入团队成员(重复幂等)。
    ///
    /// # Errors
    /// 存储故障。
    async fn add_member(&self, team_id: TeamId, user_id: UserId) -> Result<(), AppError>;

    /// 移出团队成员,返回是否命中。
    ///
    /// # Errors
    /// 存储故障。
    async fn remove_member(&self, team_id: TeamId, user_id: UserId) -> Result<bool, AppError>;

    /// 团队成员列表(加入时间正序)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_members(&self, team_id: TeamId) -> Result<Vec<TeamMember>, AppError>;
}

/// 资源级角色(项目/仓库/应用上的成员角色)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceRole {
    /// 只读。
    Read,
    /// 读写。
    Write,
    /// 管理(成员/设置)。
    Admin,
}

impl ResourceRole {
    /// 数据库存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Admin => "admin",
        }
    }

    /// 从数据库值解析。
    #[must_use]
    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value {
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }

    /// 角色是否隐含所需动作(read ⊂ write ⊂ admin)。
    #[must_use]
    pub fn implies(self, action: codeplan_kernel::authz::Action) -> bool {
        use codeplan_kernel::authz::Action;
        match action {
            Action::Read => true,
            Action::Write => self != Self::Read,
            Action::Admin => self == Self::Admin,
        }
    }
}

/// 成员条目(带展示信息)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberEntry {
    /// 用户 ID。
    pub user_id: UserId,
    /// 登录名。
    pub username: String,
    /// 显示名。
    pub display_name: String,
    /// 角色。
    pub role: ResourceRole,
}

/// 资源角色存储抽象。
#[async_trait::async_trait]
pub trait RbacStore: Send + Sync {
    /// 授予/更新用户在资源上的角色(upsert)。
    ///
    /// # Errors
    /// 存储故障。
    async fn grant(
        &self,
        user_id: UserId,
        kind: &str,
        resource_id: uuid::Uuid,
        role: ResourceRole,
    ) -> Result<(), codeplan_common::error::AppError>;

    /// 撤销用户在资源上的角色(不存在则幂等成功)。
    ///
    /// # Errors
    /// 存储故障。
    async fn revoke(
        &self,
        user_id: UserId,
        kind: &str,
        resource_id: uuid::Uuid,
    ) -> Result<(), codeplan_common::error::AppError>;

    /// 查用户在资源上的角色。
    ///
    /// # Errors
    /// 存储故障。
    async fn role_of(
        &self,
        user_id: UserId,
        kind: &str,
        resource_id: uuid::Uuid,
    ) -> Result<Option<ResourceRole>, codeplan_common::error::AppError>;

    /// 资源成员列表(含用户展示信息,JOIN org_user)。
    ///
    /// # Errors
    /// 存储故障。
    async fn members(
        &self,
        kind: &str,
        resource_id: uuid::Uuid,
    ) -> Result<Vec<MemberEntry>, codeplan_common::error::AppError>;
}

/// SSH 公钥。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshKey {
    /// 密钥 ID。
    pub id: uuid::Uuid,
    /// 所属用户。
    pub user_id: UserId,
    /// 标题。
    pub title: String,
    /// 公钥内容(OpenSSH 一行格式)。
    pub public_key: String,
    /// SHA256 指纹(`SHA256:base64`)。
    pub fingerprint: String,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// SSH 公钥仓储抽象。
#[async_trait::async_trait]
pub trait SshKeyRepository: Send + Sync {
    /// 添加公钥(指纹唯一冲突返回 `ORG-409`)。
    ///
    /// # Errors
    /// 存储故障或指纹冲突。
    async fn add(
        &self,
        user_id: UserId,
        title: &str,
        public_key: &str,
        fingerprint: &str,
    ) -> Result<SshKey, AppError>;

    /// 用户的公钥列表。
    ///
    /// # Errors
    /// 存储故障。
    async fn list_for_user(&self, user_id: UserId) -> Result<Vec<SshKey>, AppError>;

    /// 删除公钥(仅属主;返回是否删除)。
    ///
    /// # Errors
    /// 存储故障。
    async fn delete(&self, user_id: UserId, id: uuid::Uuid) -> Result<bool, AppError>;

    /// 按指纹查用户(SSH 认证)。
    ///
    /// # Errors
    /// 存储故障。
    async fn user_by_fingerprint(
        &self,
        fingerprint: &str,
    ) -> Result<Option<(UserId, String)>, AppError>;
}
