//! codeplan 组合根:装配所有业务域、基础设施与插件(L4,唯一二进制)。
//!
//! 当前为阶段 0 骨架:仅健康检查与 OpenAPI 契约端点,随批次任务逐步装配各域。

use std::sync::Arc;
use std::time::Duration;

use axum::{Json, Router, routing::get};
use serde::Serialize;
use utoipa::{OpenApi, ToSchema};

#[derive(Serialize, ToSchema)]
struct Health {
    status: String,
    version: String,
}

/// Liveness/Readiness 探针。
#[utoipa::path(get, path = "/api/health", responses((status = 200, body = Health)))]
async fn health() -> Json<Health> {
    Json(Health {
        status: "ok".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

#[derive(OpenApi)]
#[openapi(paths(health), components(schemas(Health)))]
struct ApiDoc;

/// 聚合各域 OpenAPI 片段。
fn build_openapi() -> serde_json::Value {
    let mut doc = ApiDoc::openapi();
    doc.merge(codeplan_identity::api::openapi());
    serde_json::to_value(doc).expect("OpenAPI 文档序列化失败")
}

/// 环境变量缺省值。
fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}

async fn openapi_json() -> Json<serde_json::Value> {
    Json(build_openapi())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 数据库(迁移 + 连接池)
    let db_url = env_or(
        "CODEPLAN_DATABASE_URL",
        "postgres://codeplan:codeplan-dev@localhost:5434/codeplan",
    );
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&db_url)
        .await?;
    sqlx::migrate!("../../migrations").run(&pool).await?;

    // 身份域装配
    let users: Arc<dyn codeplan_identity::domain::UserRepository> = Arc::new(
        codeplan_identity::infra::PgUserRepository::new(pool.clone()),
    );
    let jwt_secret = env_or("CODEPLAN_JWT_SECRET", "dev-secret-do-not-use-in-prod");

    // 审计槽(PG;T-106)
    let audit_sink: Arc<dyn codeplan_kernel::audit::AuditSink> =
        Arc::new(codeplan_identity::infra::PgAuditSink::new(pool.clone()));

    // LDAP(可选):登录回退 + 定时同步(T-105)
    let directory: Option<Arc<dyn codeplan_identity::ldap::LdapDirectory>> =
        codeplan_identity::ldap::LdapConfig::from_env()
            .map(|cfg| Arc::new(codeplan_identity::ldap::LdapDirectoryImpl::new(cfg)) as Arc<_>);

    let mut auth = codeplan_identity::service::AuthService::new(Arc::clone(&users), &jwt_secret);
    if let Some(dir) = &directory {
        auth = auth.with_directory(Arc::clone(dir));
    }
    auth = auth.with_audit(Arc::clone(&audit_sink));
    let auth = Arc::new(auth);

    if let Some(dir) = &directory {
        let hours: u64 = env_or("CODEPLAN_LDAP_SYNC_HOURS", "6").parse().unwrap_or(6);
        if hours > 0 {
            let dir = Arc::clone(dir);
            tokio::spawn(async move {
                let mut ticker =
                    tokio::time::interval(std::time::Duration::from_secs(hours * 3600));
                ticker.tick().await; // 启动即同步一次
                loop {
                    match dir.list_users().await {
                        Ok(list) => println!("LDAP 目录用户 {} 个(同步周期 {hours}h)", list.len()),
                        Err(e) => eprintln!("LDAP 同步失败: {e}"),
                    }
                    ticker.tick().await;
                }
            });
        }
    }

    let rbac = Arc::new(
        codeplan_identity::rbac::RbacService::new(
            Arc::new(codeplan_identity::infra::PgRbacStore::new(pool.clone())),
            Arc::clone(&users),
        )
        .with_audit(Arc::clone(&audit_sink)),
    );
    auth.bootstrap_admin_if_empty(&env_or(
        "CODEPLAN_BOOTSTRAP_ADMIN_PASSWORD",
        "codeplan-admin",
    ))
    .await?;

    let oidc = codeplan_identity::oidc::OidcConfig::from_env()
        .map(|cfg| Arc::new(codeplan_identity::oidc::OidcCtx::new(cfg)));
    let state = codeplan_identity::api::AuthState {
        auth,
        oidc,
        rbac,
        ldap: directory,
        audit: Arc::new(codeplan_identity::infra::PgAuditSink::new(pool.clone())),
    };

    let mut app = Router::new()
        .route("/api/health", get(health))
        .route("/api/docs/openapi.json", get(openapi_json))
        .merge(codeplan_identity::api::routes());
    if state.oidc.is_some() {
        app = app.merge(codeplan_identity::oidc::routes());
    }
    let app = app.with_state(state);

    let addr = env_or("CODEPLAN_ADDR", "0.0.0.0:8080");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("codeplan-server 监听 http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
