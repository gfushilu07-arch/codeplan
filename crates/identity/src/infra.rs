//! 仓储的 PostgreSQL 实现(SQLx 运行时查询,表前缀 `org_*`)。

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use codeplan_common::error::AppError;
use codeplan_kernel::ids::UserId;
use sqlx::PgPool;

use crate::DOMAIN_CODE;
use crate::domain::{NewUser, Organization, User, UserRepository, UserStatus};

/// 用户仓储的 PostgreSQL 实现。
#[derive(Clone)]
pub struct PgUserRepository {
    pool: PgPool,
}

impl PgUserRepository {
    /// 构造(共享连接池)。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// 行映射(内部)。
#[derive(sqlx::FromRow)]
struct UserRow {
    id: uuid::Uuid,
    username: String,
    email: String,
    display_name: String,
    status: String,
    password_hash: Option<String>,
    source: String,
    totp_secret: Option<String>,
    totp_enabled: bool,
    is_platform_admin: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<UserRow> for User {
    type Error = AppError;

    fn try_from(row: UserRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: UserId::from_uuid(row.id),
            username: row.username,
            email: row.email,
            display_name: row.display_name,
            status: UserStatus::from_str_opt(&row.status)
                .ok_or_else(|| AppError::internal(DOMAIN_CODE))?,
            password_hash: row.password_hash,
            source: crate::domain::UserSource::from_str_opt(&row.source)
                .unwrap_or(crate::domain::UserSource::Local),
            totp_secret: row.totp_secret,
            totp_enabled: row.totp_enabled,
            is_platform_admin: row.is_platform_admin,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[async_trait]
impl UserRepository for PgUserRepository {
    async fn create(&self, user: &NewUser) -> Result<User, AppError> {
        let password_hash = match &user.password {
            Some(plain) => Some(crate::service::hash_password(plain)?),
            None => None,
        };
        let row = sqlx::query_as::<_, UserRow>(
            "INSERT INTO org_user (id, username, email, display_name, status, password_hash, \
             source, is_platform_admin) \
             VALUES ($1, $2, $3, $4, 'active', $5, $7, $6) \
             RETURNING id, username, email, display_name, status, password_hash, source, \
             totp_secret, totp_enabled, is_platform_admin, created_at, updated_at",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(&user.username)
        .bind(&user.email)
        .bind(&user.display_name)
        .bind(&password_hash)
        .bind(user.is_platform_admin)
        .bind(user.source.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(unique_conflict)?;

        row.try_into()
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<User>, AppError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, email, display_name, status, password_hash, source, totp_secret, \
             totp_enabled, is_platform_admin, created_at, updated_at \
             FROM org_user WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| AppError::internal(DOMAIN_CODE))?;
        row.map(User::try_from).transpose()
    }

    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, AppError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, email, display_name, status, password_hash, source, totp_secret, \
             totp_enabled, is_platform_admin, created_at, updated_at \
             FROM org_user WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| AppError::internal(DOMAIN_CODE))?;
        row.map(User::try_from).transpose()
    }

    async fn count(&self) -> Result<i64, AppError> {
        let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM org_user")
            .fetch_one(&self.pool)
            .await
            .map_err(|_| AppError::internal(DOMAIN_CODE))?;
        Ok(count)
    }

    async fn upsert_ldap_user(
        &self,
        username: &str,
        email: &str,
        display_name: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO org_user (id, username, email, display_name, status, source) \
             VALUES ($1, $2, $3, $4, 'active', 'ldap') \
             ON CONFLICT (username) DO UPDATE \
             SET email = EXCLUDED.email, display_name = EXCLUDED.display_name, source = 'ldap', \
             updated_at = now()",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(username)
        .bind(email)
        .bind(display_name)
        .execute(&self.pool)
        .await
        .map_err(|_| AppError::internal(DOMAIN_CODE))?;
        Ok(())
    }

    async fn list(&self) -> Result<Vec<User>, AppError> {
        let rows = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, email, display_name, status, password_hash, source, totp_secret, \
             totp_enabled, is_platform_admin, created_at, updated_at \
             FROM org_user ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
        })?;
        rows.into_iter().map(User::try_from).collect()
    }

    async fn set_status(&self, id: UserId, status: UserStatus) -> Result<(), AppError> {
        sqlx::query("UPDATE org_user SET status = $2, updated_at = now() WHERE id = $1")
            .bind(id.as_uuid())
            .bind(status.as_str())
            .execute(&self.pool)
            .await
            .map_err(|e| {
                AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
            })?;
        Ok(())
    }

    async fn find_by_email(&self, email: &str) -> Result<Option<User>, AppError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, email, display_name, status, password_hash, source, totp_secret, \
             totp_enabled, is_platform_admin, created_at, updated_at \
             FROM org_user WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| AppError::internal(DOMAIN_CODE))?;
        row.map(User::try_from).transpose()
    }

    async fn save_totp(
        &self,
        id: UserId,
        secret: Option<String>,
        enabled: bool,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE org_user SET totp_secret = $2, totp_enabled = $3, updated_at = now() \
             WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(&secret)
        .bind(enabled)
        .execute(&self.pool)
        .await
        .map_err(|_| AppError::internal(DOMAIN_CODE))?;
        Ok(())
    }
}

/// 组织仓储的 PostgreSQL 实现。
#[derive(Clone)]
pub struct PgOrgRepository {
    pool: PgPool,
}

impl PgOrgRepository {
    /// 构造(共享连接池)。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl crate::domain::OrgRepository for PgOrgRepository {
    async fn create(&self, name: &str) -> Result<Organization, AppError> {
        let (id, created_at): (uuid::Uuid, DateTime<Utc>) = sqlx::query_as(
            "INSERT INTO org_organization (id, name) VALUES ($1, $2) RETURNING id, created_at",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(name)
        .fetch_one(&self.pool)
        .await
        .map_err(unique_conflict)?;
        Ok(Organization {
            id: crate::domain::OrgId::from_uuid(id),
            name: name.to_owned(),
            created_at,
        })
    }

    async fn list(&self) -> Result<Vec<Organization>, AppError> {
        let rows: Vec<(uuid::Uuid, String, DateTime<Utc>)> =
            sqlx::query_as("SELECT id, name, created_at FROM org_organization ORDER BY created_at")
                .fetch_all(&self.pool)
                .await
                .map_err(|_| AppError::internal(DOMAIN_CODE))?;
        Ok(rows
            .into_iter()
            .map(|(id, name, created_at)| Organization {
                id: crate::domain::OrgId::from_uuid(id),
                name,
                created_at,
            })
            .collect())
    }
}

/// 唯一键冲突 → `ORG-409`,其余 → `ORG-500`。
fn unique_conflict(e: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(db) = &e
        && db.constraint().is_some()
    {
        return AppError::conflict(DOMAIN_CODE, "名称或标识已存在");
    }
    AppError::internal(DOMAIN_CODE)
}

/// 资源角色存储的 PostgreSQL 实现。
#[derive(Clone)]
pub struct PgRbacStore {
    pool: PgPool,
}

impl PgRbacStore {
    /// 构造(共享连接池)。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl crate::domain::RbacStore for PgRbacStore {
    async fn grant(
        &self,
        user_id: UserId,
        kind: &str,
        resource_id: uuid::Uuid,
        role: crate::domain::ResourceRole,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO org_resource_role (user_id, resource_kind, resource_id, role) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (user_id, resource_kind, resource_id) \
             DO UPDATE SET role = EXCLUDED.role, granted_at = now()",
        )
        .bind(user_id.as_uuid())
        .bind(kind)
        .bind(resource_id)
        .bind(role.as_str())
        .execute(&self.pool)
        .await
        .map_err(|_| AppError::internal(DOMAIN_CODE))?;
        Ok(())
    }

    async fn revoke(
        &self,
        user_id: UserId,
        kind: &str,
        resource_id: uuid::Uuid,
    ) -> Result<(), AppError> {
        sqlx::query(
            "DELETE FROM org_resource_role \
             WHERE user_id = $1 AND resource_kind = $2 AND resource_id = $3",
        )
        .bind(user_id.as_uuid())
        .bind(kind)
        .bind(resource_id)
        .execute(&self.pool)
        .await
        .map_err(|_| AppError::internal(DOMAIN_CODE))?;
        Ok(())
    }

    async fn role_of(
        &self,
        user_id: UserId,
        kind: &str,
        resource_id: uuid::Uuid,
    ) -> Result<Option<crate::domain::ResourceRole>, AppError> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT role FROM org_resource_role \
             WHERE user_id = $1 AND resource_kind = $2 AND resource_id = $3",
        )
        .bind(user_id.as_uuid())
        .bind(kind)
        .bind(resource_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| AppError::internal(DOMAIN_CODE))?;
        Ok(row.and_then(|(role,)| crate::domain::ResourceRole::from_str_opt(&role)))
    }

    async fn members(
        &self,
        kind: &str,
        resource_id: uuid::Uuid,
    ) -> Result<Vec<crate::domain::MemberEntry>, AppError> {
        let rows: Vec<(uuid::Uuid, String, String, String)> = sqlx::query_as(
            "SELECT r.user_id, u.username, u.display_name, r.role \
             FROM org_resource_role r JOIN org_user u ON u.id = r.user_id \
             WHERE r.resource_kind = $1 AND r.resource_id = $2 ORDER BY r.granted_at",
        )
        .bind(kind)
        .bind(resource_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| AppError::internal(DOMAIN_CODE))?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, username, display_name, role)| {
                Some(crate::domain::MemberEntry {
                    user_id: UserId::from_uuid(id),
                    username,
                    display_name,
                    role: crate::domain::ResourceRole::from_str_opt(&role)?,
                })
            })
            .collect())
    }
}

/// 审计槽的 PostgreSQL 实现(org_audit_log)。
#[derive(Clone)]
pub struct PgAuditSink {
    pool: PgPool,
}

impl PgAuditSink {
    /// 构造(共享连接池)。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 最近审计记录(管理端查询,ORG-05)。
    pub async fn recent(
        &self,
        limit: i64,
    ) -> Result<Vec<codeplan_kernel::audit::AuditEvent>, AppError> {
        type AuditRow = (
            uuid::Uuid,
            Option<uuid::Uuid>,
            String,
            Option<String>,
            Option<uuid::Uuid>,
            serde_json::Value,
            chrono::DateTime<Utc>,
        );
        let rows: Vec<AuditRow> = sqlx::query_as(
            "SELECT id, actor, action, resource_kind, resource_id, detail, at \
                 FROM org_audit_log ORDER BY at DESC LIMIT $1",
        )
        .bind(limit.clamp(1, 500))
        .fetch_all(&self.pool)
        .await
        .map_err(|_| AppError::internal(DOMAIN_CODE))?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, actor, action, kind, rid, detail, at)| {
                let resource = match (kind.as_deref(), rid) {
                    (Some("system"), _) => codeplan_kernel::authz::ResourceRef::System,
                    (Some("project"), Some(r)) => codeplan_kernel::authz::ResourceRef::Project(
                        codeplan_kernel::ids::ProjectId::from_uuid(r),
                    ),
                    (Some("repo"), Some(r)) => codeplan_kernel::authz::ResourceRef::Repo(
                        codeplan_kernel::ids::RepoId::from_uuid(r),
                    ),
                    (Some("app"), Some(r)) => codeplan_kernel::authz::ResourceRef::App(
                        codeplan_kernel::ids::AppId::from_uuid(r),
                    ),
                    _ => return None,
                };
                Some(codeplan_kernel::audit::AuditEvent {
                    id,
                    actor: actor.map(UserId::from_uuid),
                    action,
                    resource,
                    detail,
                    at,
                })
            })
            .collect())
    }
}

#[async_trait]
impl codeplan_kernel::audit::AuditSink for PgAuditSink {
    async fn record(&self, event: codeplan_kernel::audit::AuditEvent) -> Result<(), String> {
        let (kind, rid) = match &event.resource {
            codeplan_kernel::authz::ResourceRef::System => (Some("system"), None),
            codeplan_kernel::authz::ResourceRef::Project(id) => {
                (Some("project"), Some(id.as_uuid()))
            }
            codeplan_kernel::authz::ResourceRef::Repo(id) => (Some("repo"), Some(id.as_uuid())),
            codeplan_kernel::authz::ResourceRef::App(id) => (Some("app"), Some(id.as_uuid())),
        };
        sqlx::query(
            "INSERT INTO org_audit_log (id, actor, action, resource_kind, resource_id, detail, at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(event.id)
        .bind(event.actor.map(|u| u.as_uuid()))
        .bind(&event.action)
        .bind(kind)
        .bind(rid)
        .bind(&event.detail)
        .bind(event.at)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
    }
}

/// SSH 公钥仓储 PG 实现。
#[derive(Clone)]
pub struct PgSshKeyStore {
    pool: PgPool,
}

impl PgSshKeyStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl crate::domain::SshKeyRepository for PgSshKeyStore {
    async fn add(
        &self,
        user_id: UserId,
        title: &str,
        public_key: &str,
        fingerprint: &str,
    ) -> Result<crate::domain::SshKey, AppError> {
        let (id, created_at): (uuid::Uuid, DateTime<Utc>) = sqlx::query_as(
            "INSERT INTO org_ssh_key (id, user_id, title, public_key, fingerprint) \
             VALUES ($1, $2, $3, $4, $5) RETURNING id, created_at",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(user_id.as_uuid())
        .bind(title)
        .bind(public_key)
        .bind(fingerprint)
        .fetch_one(&self.pool)
        .await
        .map_err(unique_conflict)?;
        Ok(crate::domain::SshKey {
            id,
            user_id,
            title: title.to_owned(),
            public_key: public_key.to_owned(),
            fingerprint: fingerprint.to_owned(),
            created_at,
        })
    }

    async fn list_for_user(&self, user_id: UserId) -> Result<Vec<crate::domain::SshKey>, AppError> {
        let rows: Vec<(uuid::Uuid, String, String, String, DateTime<Utc>)> = sqlx::query_as(
            "SELECT id, title, public_key, fingerprint, created_at FROM org_ssh_key \
             WHERE user_id = $1 ORDER BY created_at",
        )
        .bind(user_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
        })?;
        Ok(rows
            .into_iter()
            .map(
                |(id, title, public_key, fingerprint, created_at)| crate::domain::SshKey {
                    id,
                    user_id,
                    title,
                    public_key,
                    fingerprint,
                    created_at,
                },
            )
            .collect())
    }

    async fn delete(&self, user_id: UserId, id: uuid::Uuid) -> Result<bool, AppError> {
        let result = sqlx::query("DELETE FROM org_ssh_key WHERE id = $1 AND user_id = $2")
            .bind(id)
            .bind(user_id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| {
                AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
            })?;
        Ok(result.rows_affected() > 0)
    }

    async fn user_by_fingerprint(
        &self,
        fingerprint: &str,
    ) -> Result<Option<(UserId, String)>, AppError> {
        let row: Option<(uuid::Uuid, String)> = sqlx::query_as(
            "SELECT u.id, u.username FROM org_ssh_key k JOIN org_user u ON u.id = k.user_id \
             WHERE k.fingerprint = $1",
        )
        .bind(fingerprint)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
        })?;
        Ok(row.map(|(id, username)| (UserId::from_uuid(id), username)))
    }
}

/// kernel SshKeyDirectory 实现(SSH 服务认证目录)。
#[async_trait]
impl codeplan_kernel::authn::SshKeyDirectory for PgSshKeyStore {
    async fn user_by_fingerprint(
        &self,
        fingerprint: &str,
    ) -> Result<Option<codeplan_kernel::authn::SshKeyUser>, String> {
        let row: Option<(uuid::Uuid, String)> = sqlx::query_as(
            "SELECT u.id, u.username FROM org_ssh_key k JOIN org_user u ON u.id = k.user_id \
             WHERE k.fingerprint = $1",
        )
        .bind(fingerprint)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(
            row.map(|(id, username)| codeplan_kernel::authn::SshKeyUser {
                user_id: UserId::from_uuid(id),
                username,
            }),
        )
    }
}
