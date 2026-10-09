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
             is_platform_admin) \
             VALUES ($1, $2, $3, $4, 'active', $5, $6) \
             RETURNING id, username, email, display_name, status, password_hash, \
             is_platform_admin, created_at, updated_at",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(&user.username)
        .bind(&user.email)
        .bind(&user.display_name)
        .bind(&password_hash)
        .bind(user.is_platform_admin)
        .fetch_one(&self.pool)
        .await
        .map_err(unique_conflict)?;

        row.try_into()
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<User>, AppError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, email, display_name, status, password_hash, \
             is_platform_admin, created_at, updated_at FROM org_user WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| AppError::internal(DOMAIN_CODE))?;
        row.map(User::try_from).transpose()
    }

    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, AppError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, email, display_name, status, password_hash, \
             is_platform_admin, created_at, updated_at FROM org_user WHERE id = $1",
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
