//! 应用中心仓储的 PostgreSQL 实现(表前缀 `app_*`)。

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use codeplan_common::error::AppError;
use codeplan_kernel::ids::UserId;
use sqlx::PgPool;

use crate::DOMAIN_CODE;
use crate::domain::{
    AppCenterRepository, Application, ApplicationId, DeployTarget, DeployTargetId, EnvKind,
    EnvVar, EnvVarId, Environment, EnvironmentId, NewApplication, NewDeployTarget, NewEnvVar,
    NewEnvironment,
};

fn db_err(e: impl std::fmt::Display) -> AppError {
    AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
}

fn unique_conflict(e: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(db) = &e
        && db.constraint().is_some()
    {
        return AppError::conflict(DOMAIN_CODE, "标识已存在");
    }
    db_err(e)
}

/// 应用中心 PG 仓储。
pub struct PgAppCenterRepository {
    pool: PgPool,
}

impl PgAppCenterRepository {
    /// 构造。
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

type AppRow = (uuid::Uuid, String, String, String, Option<uuid::Uuid>, DateTime<Utc>, DateTime<Utc>);
type EnvRow = (uuid::Uuid, uuid::Uuid, String, String, DateTime<Utc>);
type TargetRow = (uuid::Uuid, uuid::Uuid, String, String, i32, serde_json::Value, DateTime<Utc>);
type VarRow = (uuid::Uuid, uuid::Uuid, String, String, bool, DateTime<Utc>);

fn app_from_row(r: AppRow) -> Application {
    Application {
        id: ApplicationId::from_uuid(r.0),
        key: r.1,
        name: r.2,
        description: r.3,
        owner_id: r.4.map(UserId::from_uuid),
        created_at: r.5,
        updated_at: r.6,
    }
}

fn env_from_row(r: EnvRow) -> Environment {
    Environment {
        id: EnvironmentId::from_uuid(r.0),
        app_id: ApplicationId::from_uuid(r.1),
        name: r.2,
        kind: EnvKind::parse(&r.3).unwrap_or(EnvKind::Dev),
        created_at: r.4,
    }
}

fn target_from_row(r: TargetRow) -> DeployTarget {
    DeployTarget {
        id: DeployTargetId::from_uuid(r.0),
        env_id: EnvironmentId::from_uuid(r.1),
        name: r.2,
        host: r.3,
        port: r.4,
        labels: r
            .5
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
        created_at: r.6,
    }
}

fn var_from_row(r: VarRow) -> EnvVar {
    EnvVar {
        id: EnvVarId::from_uuid(r.0),
        env_id: EnvironmentId::from_uuid(r.1),
        key: r.2,
        value: r.3,
        is_secret: r.4,
        created_at: r.5,
    }
}

const APP_COLS: &str = "id, key, name, description, owner_id, created_at, updated_at";
const ENV_COLS: &str = "id, app_id, name, kind, created_at";

#[async_trait]
impl AppCenterRepository for PgAppCenterRepository {
    async fn create_app(&self, app: &NewApplication) -> Result<Application, AppError> {
        let row = sqlx::query_as::<_, AppRow>(&format!(
            "INSERT INTO app_application (id, key, name, description, owner_id) \
             VALUES ($1, $2, $3, $4, $5) RETURNING {APP_COLS}"
        ))
        .bind(uuid::Uuid::new_v4())
        .bind(&app.key)
        .bind(&app.name)
        .bind(&app.description)
        .bind(app.owner_id.map(|u| u.as_uuid()))
        .fetch_one(&self.pool)
        .await
        .map_err(unique_conflict)?;
        Ok(app_from_row(row))
    }

    async fn list_apps(&self) -> Result<Vec<Application>, AppError> {
        let rows = sqlx::query_as::<_, AppRow>(&format!(
            "SELECT {APP_COLS} FROM app_application ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(app_from_row).collect())
    }

    async fn find_app_by_key(&self, key: &str) -> Result<Option<Application>, AppError> {
        let row = sqlx::query_as::<_, AppRow>(&format!(
            "SELECT {APP_COLS} FROM app_application WHERE key = $1"
        ))
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(app_from_row))
    }

    async fn find_app_by_id(&self, id: ApplicationId) -> Result<Option<Application>, AppError> {
        let row = sqlx::query_as::<_, AppRow>(&format!(
            "SELECT {APP_COLS} FROM app_application WHERE id = $1"
        ))
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(app_from_row))
    }

    async fn create_env(&self, env: &NewEnvironment) -> Result<Environment, AppError> {
        let row = sqlx::query_as::<_, EnvRow>(&format!(
            "INSERT INTO app_environment (id, app_id, name, kind) VALUES ($1, $2, $3, $4) \
             RETURNING {ENV_COLS}"
        ))
        .bind(uuid::Uuid::new_v4())
        .bind(env.app_id.as_uuid())
        .bind(&env.name)
        .bind(env.kind.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(unique_conflict)?;
        Ok(env_from_row(row))
    }

    async fn list_envs(&self, app_id: ApplicationId) -> Result<Vec<Environment>, AppError> {
        let rows = sqlx::query_as::<_, EnvRow>(&format!(
            "SELECT {ENV_COLS} FROM app_environment WHERE app_id = $1 ORDER BY created_at"
        ))
        .bind(app_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(env_from_row).collect())
    }

    async fn find_env(&self, id: EnvironmentId) -> Result<Option<Environment>, AppError> {
        let row = sqlx::query_as::<_, EnvRow>(&format!(
            "SELECT {ENV_COLS} FROM app_environment WHERE id = $1"
        ))
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(env_from_row))
    }

    async fn delete_env(&self, id: EnvironmentId) -> Result<bool, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        for sql in [
            "DELETE FROM app_env_var WHERE env_id = $1",
            "DELETE FROM app_deploy_target WHERE env_id = $1",
            "DELETE FROM app_environment WHERE id = $1",
        ] {
            let res = sqlx::query(sql)
                .bind(id.as_uuid())
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            if sql.starts_with("DELETE FROM app_environment") {
                let hit = res.rows_affected() > 0;
                tx.commit().await.map_err(db_err)?;
                return Ok(hit);
            }
        }
        Ok(false)
    }

    async fn add_target(&self, t: &NewDeployTarget) -> Result<DeployTarget, AppError> {
        let row = sqlx::query_as::<_, TargetRow>(
            "INSERT INTO app_deploy_target (id, env_id, name, host, port, labels) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             RETURNING id, env_id, name, host, port, labels, created_at",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(t.env_id.as_uuid())
        .bind(&t.name)
        .bind(&t.host)
        .bind(t.port)
        .bind(serde_json::json!(t.labels))
        .fetch_one(&self.pool)
        .await
        .map_err(unique_conflict)?;
        Ok(target_from_row(row))
    }

    async fn list_targets(&self, env_id: EnvironmentId) -> Result<Vec<DeployTarget>, AppError> {
        let rows = sqlx::query_as::<_, TargetRow>(
            "SELECT id, env_id, name, host, port, labels, created_at FROM app_deploy_target \
             WHERE env_id = $1 ORDER BY created_at",
        )
        .bind(env_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(target_from_row).collect())
    }

    async fn remove_target(&self, id: DeployTargetId) -> Result<bool, AppError> {
        let res = sqlx::query("DELETE FROM app_deploy_target WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(res.rows_affected() > 0)
    }

    async fn upsert_var(&self, v: &NewEnvVar) -> Result<EnvVar, AppError> {
        let row = sqlx::query_as::<_, VarRow>(
            "INSERT INTO app_env_var (id, env_id, key, value, is_secret) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (env_id, key) DO UPDATE SET value = EXCLUDED.value, \
               is_secret = EXCLUDED.is_secret \
             RETURNING id, env_id, key, value, is_secret, created_at",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(v.env_id.as_uuid())
        .bind(&v.key)
        .bind(&v.value)
        .bind(v.is_secret)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(var_from_row(row))
    }

    async fn list_vars(&self, env_id: EnvironmentId) -> Result<Vec<EnvVar>, AppError> {
        let rows = sqlx::query_as::<_, VarRow>(
            "SELECT id, env_id, key, value, is_secret, created_at FROM app_env_var \
             WHERE env_id = $1 ORDER BY key",
        )
        .bind(env_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(var_from_row).collect())
    }

    async fn delete_var(&self, id: EnvVarId) -> Result<bool, AppError> {
        let res = sqlx::query("DELETE FROM app_env_var WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(res.rows_affected() > 0)
    }

    async fn link_repo(&self, app_id: ApplicationId, repo_id: uuid::Uuid) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO app_repo_link (app_id, repo_id) VALUES ($1, $2) \
             ON CONFLICT (app_id, repo_id) DO NOTHING",
        )
        .bind(app_id.as_uuid())
        .bind(repo_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn unlink_repo(
        &self,
        app_id: ApplicationId,
        repo_id: uuid::Uuid,
    ) -> Result<bool, AppError> {
        let res = sqlx::query("DELETE FROM app_repo_link WHERE app_id = $1 AND repo_id = $2")
            .bind(app_id.as_uuid())
            .bind(repo_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(res.rows_affected() > 0)
    }

    async fn list_repo_ids(&self, app_id: ApplicationId) -> Result<Vec<uuid::Uuid>, AppError> {
        let rows: Vec<(uuid::Uuid,)> =
            sqlx::query_as("SELECT repo_id FROM app_repo_link WHERE app_id = $1 ORDER BY added_at")
                .bind(app_id.as_uuid())
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn repo_full_names(
        &self,
        ids: &[uuid::Uuid],
    ) -> Result<Vec<(uuid::Uuid, String)>, AppError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        // 只读投影:仅取展示名,不写他域数据
        let rows: Vec<(uuid::Uuid, String)> =
            sqlx::query_as("SELECT id, full_name FROM repo_repository WHERE id = ANY($1)")
                .bind(ids)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?;
        Ok(rows)
    }
}

// ---------- 服务规格(T-312) ----------

type SpecRow = (i32, serde_json::Value, String, Option<uuid::Uuid>, DateTime<Utc>);

fn spec_from_row(r: SpecRow) -> Option<crate::domain::SpecVersion> {
    let spec: crate::domain::ServiceSpec = serde_json::from_value(r.1).ok()?;
    Some(crate::domain::SpecVersion {
        version: r.0,
        spec,
        source: r.2,
        created_by: r.3.map(|u| u.to_string()),
        created_at: r.4.to_rfc3339(),
    })
}

#[async_trait]
impl crate::domain::SpecRepository for PgAppCenterRepository {
    async fn ensure_service(
        &self,
        app_id: crate::domain::ApplicationId,
        env_id: crate::domain::EnvironmentId,
    ) -> Result<(crate::domain::ServiceId, i32), AppError> {
        let row: (uuid::Uuid, i32) = sqlx::query_as(
            "INSERT INTO app_service (id, app_id, env_id) VALUES ($1, $2, $3) \
             ON CONFLICT (app_id, env_id) DO UPDATE SET updated_at = now() \
             RETURNING id, current_version",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(app_id.as_uuid())
        .bind(env_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok((crate::domain::ServiceId::from_uuid(row.0), row.1))
    }

    async fn append_spec(
        &self,
        service_id: crate::domain::ServiceId,
        version: i32,
        spec: &crate::domain::ServiceSpec,
        source: &str,
        created_by: Option<UserId>,
    ) -> Result<(), AppError> {
        let value = serde_json::to_value(spec).map_err(db_err)?;
        sqlx::query(
            "INSERT INTO app_service_spec (id, service_id, version, spec, source, created_by) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(service_id.as_uuid())
        .bind(version)
        .bind(value)
        .bind(source)
        .bind(created_by.map(|u| u.as_uuid()))
        .execute(&self.pool)
        .await
        .map_err(unique_conflict)?;
        Ok(())
    }

    async fn set_current_version(
        &self,
        service_id: crate::domain::ServiceId,
        version: i32,
    ) -> Result<(), AppError> {
        sqlx::query("UPDATE app_service SET current_version = $2, updated_at = now() WHERE id = $1")
            .bind(service_id.as_uuid())
            .bind(version)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn current_spec(
        &self,
        service_id: crate::domain::ServiceId,
    ) -> Result<Option<crate::domain::SpecVersion>, AppError> {
        let row: Option<SpecRow> = sqlx::query_as(
            "SELECT s.version, s.spec, s.source, s.created_by, s.created_at \
             FROM app_service_spec s JOIN app_service v ON v.id = s.service_id \
             WHERE s.service_id = $1 AND s.version = v.current_version",
        )
        .bind(service_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.and_then(spec_from_row))
    }

    async fn list_spec_versions(
        &self,
        service_id: crate::domain::ServiceId,
    ) -> Result<Vec<crate::domain::SpecVersion>, AppError> {
        let rows: Vec<SpecRow> = sqlx::query_as(
            "SELECT version, spec, source, created_by, created_at FROM app_service_spec \
             WHERE service_id = $1 ORDER BY version DESC",
        )
        .bind(service_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().filter_map(spec_from_row).collect())
    }

    async fn spec_at(
        &self,
        service_id: crate::domain::ServiceId,
        version: i32,
    ) -> Result<Option<crate::domain::SpecVersion>, AppError> {
        let row: Option<SpecRow> = sqlx::query_as(
            "SELECT version, spec, source, created_by, created_at FROM app_service_spec \
             WHERE service_id = $1 AND version = $2",
        )
        .bind(service_id.as_uuid())
        .bind(version)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.and_then(spec_from_row))
    }
}

// ---------- 配置与密钥(T-317) ----------

type ConfigRow = (i32, serde_json::Value, String, Option<uuid::Uuid>, DateTime<Utc>);

fn config_from_row(r: ConfigRow) -> Option<crate::domain::StoredConfigVersion> {
    let items: Vec<crate::domain::ConfigItem> = serde_json::from_value(r.1).ok()?;
    Some((r.0, items, r.2, r.3.map(|u| u.to_string()), r.4.to_rfc3339()))
}

#[async_trait]
impl crate::domain::ConfigRepository for PgAppCenterRepository {
    async fn ensure_config(
        &self,
        app_id: crate::domain::ApplicationId,
        env_id: crate::domain::EnvironmentId,
    ) -> Result<(crate::domain::ConfigId, i32), AppError> {
        let row: (uuid::Uuid, i32) = sqlx::query_as(
            "INSERT INTO app_config (id, app_id, env_id) VALUES ($1, $2, $3) \
             ON CONFLICT (app_id, env_id) DO UPDATE SET updated_at = now() \
             RETURNING id, current_version",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(app_id.as_uuid())
        .bind(env_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok((crate::domain::ConfigId::from_uuid(row.0), row.1))
    }

    async fn append_config(
        &self,
        config_id: crate::domain::ConfigId,
        version: i32,
        items: &[crate::domain::ConfigItem],
        source: &str,
        created_by: Option<UserId>,
    ) -> Result<(), AppError> {
        let value = serde_json::to_value(items).map_err(db_err)?;
        sqlx::query(
            "INSERT INTO app_config_version (id, config_id, version, items, source, created_by) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(config_id.as_uuid())
        .bind(version)
        .bind(value)
        .bind(source)
        .bind(created_by.map(|u| u.as_uuid()))
        .execute(&self.pool)
        .await
        .map_err(unique_conflict)?;
        Ok(())
    }

    async fn set_current_config_version(
        &self,
        config_id: crate::domain::ConfigId,
        version: i32,
    ) -> Result<(), AppError> {
        sqlx::query("UPDATE app_config SET current_version = $2, updated_at = now() WHERE id = $1")
            .bind(config_id.as_uuid())
            .bind(version)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn current_config(
        &self,
        config_id: crate::domain::ConfigId,
    ) -> Result<Option<crate::domain::StoredConfigVersion>, AppError> {
        let row: Option<ConfigRow> = sqlx::query_as(
            "SELECT v.version, v.items, v.source, v.created_by, v.created_at \
             FROM app_config_version v JOIN app_config c ON c.id = v.config_id \
             WHERE v.config_id = $1 AND v.version = c.current_version",
        )
        .bind(config_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.and_then(config_from_row))
    }

    async fn list_config_versions(
        &self,
        config_id: crate::domain::ConfigId,
    ) -> Result<Vec<crate::domain::StoredConfigVersion>, AppError> {
        let rows: Vec<ConfigRow> = sqlx::query_as(
            "SELECT version, items, source, created_by, created_at FROM app_config_version \
             WHERE config_id = $1 ORDER BY version DESC",
        )
        .bind(config_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().filter_map(config_from_row).collect())
    }
}

// ---------- 域名绑定(T-315) ----------

type DomainRow = (uuid::Uuid, uuid::Uuid, String, String, bool, String, DateTime<Utc>);

fn domain_from_row(r: DomainRow) -> crate::domain::AppDomain {
    crate::domain::AppDomain {
        id: crate::domain::DomainId::from_uuid(r.0),
        env_id: crate::domain::EnvironmentId::from_uuid(r.1),
        host: r.2,
        kind: crate::domain::DomainKind::parse(&r.3).unwrap_or(crate::domain::DomainKind::Custom),
        tls_enabled: r.4,
        cert_ref: r.5,
        created_at: r.6,
    }
}

const DOMAIN_COLS: &str = "id, env_id, host, kind, tls_enabled, cert_ref, created_at";

#[async_trait]
impl crate::domain::DomainRepository for PgAppCenterRepository {
    async fn bind(
        &self,
        d: &crate::domain::NewDomain,
    ) -> Result<crate::domain::AppDomain, AppError> {
        let row = sqlx::query_as::<_, DomainRow>(&format!(
            "INSERT INTO app_domain (id, env_id, host, kind, tls_enabled) \
             VALUES ($1, $2, $3, $4, $5) RETURNING {DOMAIN_COLS}"
        ))
        .bind(uuid::Uuid::new_v4())
        .bind(d.env_id.as_uuid())
        .bind(&d.host)
        .bind(d.kind.as_str())
        .bind(d.tls_enabled)
        .fetch_one(&self.pool)
        .await
        .map_err(unique_conflict)?;
        Ok(domain_from_row(row))
    }

    async fn list_by_env(
        &self,
        env_id: crate::domain::EnvironmentId,
    ) -> Result<Vec<crate::domain::AppDomain>, AppError> {
        let rows = sqlx::query_as::<_, DomainRow>(&format!(
            "SELECT {DOMAIN_COLS} FROM app_domain WHERE env_id = $1 ORDER BY created_at"
        ))
        .bind(env_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(domain_from_row).collect())
    }

    async fn find_by_host(
        &self,
        host: &str,
    ) -> Result<Option<crate::domain::AppDomain>, AppError> {
        let row = sqlx::query_as::<_, DomainRow>(&format!(
            "SELECT {DOMAIN_COLS} FROM app_domain WHERE host = $1"
        ))
        .bind(host)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(domain_from_row))
    }

    async fn unbind(&self, id: crate::domain::DomainId) -> Result<bool, AppError> {
        let res = sqlx::query("DELETE FROM app_domain WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(res.rows_affected() > 0)
    }

    async fn set_cert_ref(
        &self,
        id: crate::domain::DomainId,
        cert_ref: &str,
    ) -> Result<(), AppError> {
        sqlx::query("UPDATE app_domain SET cert_ref = $2 WHERE id = $1")
            .bind(id.as_uuid())
            .bind(cert_ref)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

// ---------- 证书(T-316) ----------

type CertRow = (
    uuid::Uuid,
    uuid::Uuid,
    String,
    serde_json::Value,
    String,
    String,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
);

fn cert_from_row(r: CertRow) -> crate::domain::Certificate {
    crate::domain::Certificate {
        id: crate::domain::CertificateId::from_uuid(r.0),
        env_id: crate::domain::EnvironmentId::from_uuid(r.1),
        cert_ref: r.2,
        domains: r
            .3
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
        provider: r.4,
        status: r.5,
        not_after: r.6,
        issued_at: r.7,
    }
}

const CERT_COLS: &str =
    "id, env_id, cert_ref, domains, provider, status, not_after, issued_at";

#[async_trait]
impl crate::domain::CertificateRepository for PgAppCenterRepository {
    async fn upsert_cert(
        &self,
        env_id: crate::domain::EnvironmentId,
        cert_ref: &str,
        domains: &[String],
        provider: &str,
        status: &str,
        not_after: Option<DateTime<Utc>>,
    ) -> Result<crate::domain::Certificate, AppError> {
        let row = sqlx::query_as::<_, CertRow>(&format!(
            "INSERT INTO app_certificate (id, env_id, cert_ref, domains, provider, status, not_after) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (cert_ref) DO UPDATE SET domains = EXCLUDED.domains, \
               provider = EXCLUDED.provider, status = EXCLUDED.status, \
               not_after = EXCLUDED.not_after, issued_at = now() \
             RETURNING {CERT_COLS}"
        ))
        .bind(uuid::Uuid::new_v4())
        .bind(env_id.as_uuid())
        .bind(cert_ref)
        .bind(serde_json::json!(domains))
        .bind(provider)
        .bind(status)
        .bind(not_after)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(cert_from_row(row))
    }

    async fn list_certs(
        &self,
        env_id: crate::domain::EnvironmentId,
    ) -> Result<Vec<crate::domain::Certificate>, AppError> {
        let rows = sqlx::query_as::<_, CertRow>(&format!(
            "SELECT {CERT_COLS} FROM app_certificate WHERE env_id = $1 ORDER BY issued_at DESC"
        ))
        .bind(env_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(cert_from_row).collect())
    }

    async fn expiring_before(
        &self,
        before: DateTime<Utc>,
    ) -> Result<Vec<crate::domain::Certificate>, AppError> {
        let rows = sqlx::query_as::<_, CertRow>(&format!(
            "SELECT {CERT_COLS} FROM app_certificate \
             WHERE status = 'issued' AND not_after IS NOT NULL AND not_after <= $1 \
             ORDER BY not_after"
        ))
        .bind(before)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(cert_from_row).collect())
    }

    async fn find_cert_by_ref(
        &self,
        cert_ref: &str,
    ) -> Result<Option<crate::domain::Certificate>, AppError> {
        let row = sqlx::query_as::<_, CertRow>(&format!(
            "SELECT {CERT_COLS} FROM app_certificate WHERE cert_ref = $1"
        ))
        .bind(cert_ref)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(cert_from_row))
    }
}

// ---------- 部署主机(T-311) ----------

type HostRow = (
    uuid::Uuid,
    String,
    bool,
    String,
    String,
    serde_json::Value,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
);

fn host_from_row(r: HostRow) -> crate::domain::DeployHost {
    let registered = r.2;
    let last = r.6;
    let status = crate::domain::status_from_heartbeat(registered, last, Utc::now());
    crate::domain::DeployHost {
        id: crate::domain::DeployHostId::from_uuid(r.0),
        name: r.1,
        registered,
        hostname: r.3,
        agent_version: r.4,
        labels: r
            .5
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
        status,
        last_heartbeat_at: last,
        created_at: r.7,
    }
}

const HOST_COLS: &str = "id, name, registered, hostname, agent_version, labels, last_heartbeat_at, created_at";

#[async_trait]
impl crate::domain::DeployHostRepository for PgAppCenterRepository {
    async fn create_host(
        &self,
        name: &str,
        token_hash: &str,
        labels: &[String],
    ) -> Result<crate::domain::DeployHost, AppError> {
        let row = sqlx::query_as::<_, HostRow>(&format!(
            "INSERT INTO app_deploy_host (id, name, token_hash, labels) \
             VALUES ($1, $2, $3, $4) RETURNING {HOST_COLS}"
        ))
        .bind(uuid::Uuid::new_v4())
        .bind(name)
        .bind(token_hash)
        .bind(serde_json::json!(labels))
        .fetch_one(&self.pool)
        .await
        .map_err(unique_conflict)?;
        Ok(host_from_row(row))
    }

    async fn list_hosts(&self) -> Result<Vec<crate::domain::DeployHost>, AppError> {
        let rows = sqlx::query_as::<_, HostRow>(&format!(
            "SELECT {HOST_COLS} FROM app_deploy_host ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(host_from_row).collect())
    }

    async fn find_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<crate::domain::DeployHost>, AppError> {
        let row = sqlx::query_as::<_, HostRow>(&format!(
            "SELECT {HOST_COLS} FROM app_deploy_host WHERE token_hash = $1"
        ))
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(host_from_row))
    }

    async fn find_host(
        &self,
        id: crate::domain::DeployHostId,
    ) -> Result<Option<crate::domain::DeployHost>, AppError> {
        let row = sqlx::query_as::<_, HostRow>(&format!(
            "SELECT {HOST_COLS} FROM app_deploy_host WHERE id = $1"
        ))
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(host_from_row))
    }

    async fn mark_registered(
        &self,
        id: crate::domain::DeployHostId,
        hostname: &str,
        agent_version: &str,
        labels: &[String],
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE app_deploy_host SET registered = TRUE, hostname = $2, agent_version = $3, \
             labels = $4, status = 'online', last_heartbeat_at = now() WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(hostname)
        .bind(agent_version)
        .bind(serde_json::json!(labels))
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn touch_heartbeat(&self, id: crate::domain::DeployHostId) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE app_deploy_host SET status = 'online', last_heartbeat_at = now() WHERE id = $1",
        )
        .bind(id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }
}

// ---------- 部署执行(T-313) ----------

type DeployRow = (
    uuid::Uuid,
    uuid::Uuid,
    String,
    Option<i32>,
    String,
    serde_json::Value,
    serde_json::Value,
    String,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

fn deploy_from_row(r: DeployRow) -> crate::domain::Deployment {
    crate::domain::Deployment {
        id: crate::domain::DeploymentId::from_uuid(r.0),
        env_id: crate::domain::EnvironmentId::from_uuid(r.1),
        app_key: r.2,
        spec_version: r.3,
        status: crate::domain::DeploymentStatus::parse(&r.4)
            .unwrap_or(crate::domain::DeploymentStatus::Pending),
        target_hosts: r
            .5
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| uuid::Uuid::parse_str(s).ok())
                    .map(crate::domain::DeployHostId::from_uuid)
                    .collect()
            })
            .unwrap_or_default(),
        results: r
            .6
            .as_object()
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                    .collect()
            })
            .unwrap_or_default(),
        detail: r.7,
        created_at: r.8,
        finished_at: r.9,
    }
}

const DEPLOY_COLS: &str = "id, env_id, app_key, spec_version, status, target_hosts, results, detail, created_at, finished_at";

#[async_trait]
impl crate::domain::DeploymentRepository for PgAppCenterRepository {
    async fn create_deployment(
        &self,
        env_id: crate::domain::EnvironmentId,
        app_key: &str,
        spec_version: Option<i32>,
        target_hosts: &[crate::domain::DeployHostId],
        detail: &str,
    ) -> Result<crate::domain::Deployment, AppError> {
        let hosts: Vec<String> = target_hosts
            .iter()
            .map(|h| h.as_uuid().to_string())
            .collect();
        let row = sqlx::query_as::<_, DeployRow>(&format!(
            "INSERT INTO app_deployment (id, env_id, app_key, spec_version, target_hosts, detail) \
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING {DEPLOY_COLS}"
        ))
        .bind(uuid::Uuid::new_v4())
        .bind(env_id.as_uuid())
        .bind(app_key)
        .bind(spec_version)
        .bind(serde_json::json!(hosts))
        .bind(detail)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(deploy_from_row(row))
    }

    async fn record_command(
        &self,
        command_id: uuid::Uuid,
        deployment_id: crate::domain::DeploymentId,
        host_id: crate::domain::DeployHostId,
        kind: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO app_command (id, deployment_id, host_id, kind) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(command_id)
        .bind(deployment_id.as_uuid())
        .bind(host_id.as_uuid())
        .bind(kind)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn deployment_by_command(
        &self,
        command_id: uuid::Uuid,
    ) -> Result<Option<(crate::domain::Deployment, crate::domain::DeployHostId)>, AppError> {
        let row: Option<(uuid::Uuid, uuid::Uuid)> =
            sqlx::query_as("SELECT deployment_id, host_id FROM app_command WHERE id = $1")
                .bind(command_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        let Some((deployment_id, host_id)) = row else {
            return Ok(None);
        };
        let Some(d) = self
            .find_deployment(crate::domain::DeploymentId::from_uuid(deployment_id))
            .await?
        else {
            return Ok(None);
        };
        Ok(Some((
            d,
            crate::domain::DeployHostId::from_uuid(host_id),
        )))
    }

    async fn record_result(
        &self,
        deployment_id: crate::domain::DeploymentId,
        host_id: crate::domain::DeployHostId,
        result: &str,
    ) -> Result<crate::domain::Deployment, AppError> {
        // 先写回执,再按全部目标汇总状态(未到齐保持 pending)
        sqlx::query(
            "UPDATE app_deployment SET results = jsonb_set(results, ARRAY[$2], to_jsonb($3::text)) \
             WHERE id = $1",
        )
        .bind(deployment_id.as_uuid())
        .bind(host_id.as_uuid().to_string())
        .bind(result)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        let Some(current) = self.find_deployment(deployment_id).await? else {
            return Err(AppError::not_found(DOMAIN_CODE, "部署记录不存在"));
        };
        let status = crate::domain::aggregate_status(&current.target_hosts, &current.results);
        sqlx::query(
            "UPDATE app_deployment SET status = $2, \
             finished_at = CASE WHEN $2 = 'pending' THEN NULL ELSE now() END WHERE id = $1",
        )
        .bind(deployment_id.as_uuid())
        .bind(status.as_str())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.find_deployment(deployment_id)
            .await?
            .ok_or_else(|| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!("读回失败")))
    }

    async fn list_deployments(
        &self,
        env_id: crate::domain::EnvironmentId,
        limit: i64,
    ) -> Result<Vec<crate::domain::Deployment>, AppError> {
        let rows = sqlx::query_as::<_, DeployRow>(&format!(
            "SELECT {DEPLOY_COLS} FROM app_deployment WHERE env_id = $1 \
             ORDER BY created_at DESC LIMIT $2"
        ))
        .bind(env_id.as_uuid())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows.into_iter().map(deploy_from_row).collect())
    }

    async fn find_deployment(
        &self,
        id: crate::domain::DeploymentId,
    ) -> Result<Option<crate::domain::Deployment>, AppError> {
        let row = sqlx::query_as::<_, DeployRow>(&format!(
            "SELECT {DEPLOY_COLS} FROM app_deployment WHERE id = $1"
        ))
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(deploy_from_row))
    }

    async fn expire_pending(&self, before: DateTime<Utc>) -> Result<usize, AppError> {
        let res = sqlx::query(
            "UPDATE app_deployment SET status = 'failure', finished_at = now(), \
             detail = detail || ' · 回执超时(主机掉线或 agent 未响应)' \
             WHERE status = 'pending' AND created_at <= $1",
        )
        .bind(before)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(res.rows_affected() as usize)
    }
}

