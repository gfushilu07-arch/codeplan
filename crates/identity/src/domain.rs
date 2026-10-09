//! 身份域实体与仓储 trait(纯领域,不依赖 axum/sqlx)。

use chrono::{DateTime, Utc};
use codeplan_common::define_id;
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

/// 组织仓储抽象。
#[async_trait::async_trait]
pub trait OrgRepository: Send + Sync {
    /// 创建组织(名称唯一冲突应返回 `ORG-409`)。
    ///
    /// # Errors
    /// 存储故障或唯一键冲突。
    async fn create(&self, name: &str) -> Result<Organization, codeplan_common::error::AppError>;

    /// 全量组织列表(企业内部量级可控)。
    ///
    /// # Errors
    /// 存储故障。
    async fn list(&self) -> Result<Vec<Organization>, codeplan_common::error::AppError>;
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
