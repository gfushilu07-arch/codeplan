//! PAT 与 GPG 公钥的 PG 实现(T-210)。

use chrono::{DateTime, Utc};
use codeplan_common::error::AppError;
use codeplan_kernel::ids::UserId;
use sqlx::{PgPool, Row};

use crate::gpg::{GpgKey, GpgKeyRepository, NewGpgKey};
use crate::pat::{NewPat, PatId, PatStore, PersonalAccessToken};

const DC: &str = "ORG";

fn db_err(e: impl std::fmt::Display) -> AppError {
    AppError::internal(DC).with_details(serde_json::json!(e.to_string()))
}

const PAT_COLS: &str = "id, user_id, name, token_hash, prefix, scopes, expires_at, last_used_at, revoked_at, created_at";

fn pat_row(row: sqlx::postgres::PgRow) -> PersonalAccessToken {
    PersonalAccessToken {
        id: PatId::from_uuid(row.get::<uuid::Uuid, _>("id")),
        user_id: UserId::from_uuid(row.get::<uuid::Uuid, _>("user_id")),
        name: row.get("name"),
        token_hash: row.get("token_hash"),
        prefix: row.get("prefix"),
        scopes: row.get("scopes"),
        expires_at: row.try_get("expires_at").ok().flatten(),
        last_used_at: row.try_get("last_used_at").ok().flatten(),
        revoked_at: row.try_get("revoked_at").ok().flatten(),
        created_at: row.get("created_at"),
    }
}

/// PAT PG 实现。
pub struct PgPatStore {
    pool: PgPool,
}

impl PgPatStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl PatStore for PgPatStore {
    async fn create(&self, pat: &NewPat) -> Result<PersonalAccessToken, AppError> {
        let row = sqlx::query(&format!(
            "INSERT INTO org_personal_access_token
             (id, user_id, name, token_hash, prefix, scopes, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING {PAT_COLS}"
        ))
        .bind(PatId::generate().as_uuid())
        .bind(pat.user_id.as_uuid())
        .bind(&pat.name)
        .bind(&pat.token_hash)
        .bind(&pat.prefix)
        .bind(&pat.scopes)
        .bind(pat.expires_at)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(pat_row(row))
    }

    async fn find_by_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<PersonalAccessToken>, AppError> {
        let row = sqlx::query(&format!(
            "SELECT {PAT_COLS} FROM org_personal_access_token WHERE token_hash = $1"
        ))
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(pat_row))
    }

    async fn list_by_user(&self, user_id: UserId) -> Result<Vec<PersonalAccessToken>, AppError> {
        let rows = sqlx::query(&format!(
            "SELECT {PAT_COLS} FROM org_personal_access_token
             WHERE user_id = $1 ORDER BY created_at DESC"
        ))
        .bind(user_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(pat_row).collect())
    }

    async fn revoke(&self, id: PatId, user_id: UserId) -> Result<(), AppError> {
        let n = sqlx::query(
            "UPDATE org_personal_access_token SET revoked_at = now()
             WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
        )
        .bind(id.as_uuid())
        .bind(user_id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(db_err)?
        .rows_affected();
        if n == 0 {
            return Err(AppError::not_found(DC, "令牌不存在或已吊销"));
        }
        Ok(())
    }

    async fn touch(&self, id: PatId) {
        let _ =
            sqlx::query("UPDATE org_personal_access_token SET last_used_at = now() WHERE id = $1")
                .bind(id.as_uuid())
                .execute(&self.pool)
                .await;
    }
}

// ---------- GPG 公钥 ----------

/// GPG 公钥 PG 实现。
pub struct PgGpgKeyStore {
    pool: PgPool,
}

impl PgGpgKeyStore {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const GPG_COLS: &str = "id, user_id, fingerprint, public_key, email, created_at";

fn gpg_row(row: sqlx::postgres::PgRow) -> GpgKey {
    GpgKey {
        id: crate::gpg::GpgKeyId::from_uuid(row.get::<uuid::Uuid, _>("id")),
        user_id: UserId::from_uuid(row.get::<uuid::Uuid, _>("user_id")),
        fingerprint: row.get("fingerprint"),
        public_key: row.get("public_key"),
        email: row.get("email"),
        created_at: row.get::<DateTime<Utc>, _>("created_at"),
    }
}

#[async_trait::async_trait]
impl GpgKeyRepository for PgGpgKeyStore {
    async fn upload(&self, key: &NewGpgKey) -> Result<GpgKey, AppError> {
        let row = sqlx::query(&format!(
            "INSERT INTO org_user_gpg_key (id, user_id, fingerprint, public_key, email)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (fingerprint) DO UPDATE SET user_id = $2
             RETURNING {GPG_COLS}"
        ))
        .bind(crate::gpg::GpgKeyId::generate().as_uuid())
        .bind(key.user_id.as_uuid())
        .bind(&key.fingerprint)
        .bind(&key.public_key)
        .bind(&key.email)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(gpg_row(row))
    }

    async fn list_by_user(&self, user_id: UserId) -> Result<Vec<GpgKey>, AppError> {
        let rows = sqlx::query(&format!(
            "SELECT {GPG_COLS} FROM org_user_gpg_key WHERE user_id = $1 ORDER BY created_at"
        ))
        .bind(user_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(gpg_row).collect())
    }

    async fn all(&self) -> Result<Vec<GpgKey>, AppError> {
        let rows = sqlx::query(&format!(
            "SELECT {GPG_COLS} FROM org_user_gpg_key ORDER BY created_at"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(gpg_row).collect())
    }

    async fn delete(&self, id: crate::gpg::GpgKeyId, user_id: UserId) -> Result<(), AppError> {
        let n = sqlx::query("DELETE FROM org_user_gpg_key WHERE id = $1 AND user_id = $2")
            .bind(id.as_uuid())
            .bind(user_id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(AppError::not_found(DC, "公钥不存在"));
        }
        Ok(())
    }
}

/// kernel GpgKeyDirectory 实现(T-210 签名验证目录)。
#[async_trait::async_trait]
impl codeplan_kernel::authn::GpgKeyDirectory for PgGpgKeyStore {
    async fn all_keys(&self) -> Result<Vec<codeplan_kernel::authn::GpgKeyEntry>, String> {
        let all = self.all().await.map_err(|e| e.to_string())?;
        Ok(all
            .into_iter()
            .map(|k| codeplan_kernel::authn::GpgKeyEntry {
                user_id: k.user_id,
                key_id: k.fingerprint,
                armor: k.public_key,
            })
            .collect())
    }
}
