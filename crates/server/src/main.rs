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
    let auth = Arc::new(codeplan_identity::service::AuthService::new(
        Arc::clone(&users),
        &jwt_secret,
    ));
    let rbac = Arc::new(codeplan_identity::rbac::RbacService::new(
        Arc::new(codeplan_identity::infra::PgRbacStore::new(pool.clone())),
        users,
    ));
    auth.bootstrap_admin_if_empty(&env_or(
        "CODEPLAN_BOOTSTRAP_ADMIN_PASSWORD",
        "codeplan-admin",
    ))
    .await?;

    let oidc = codeplan_identity::oidc::OidcConfig::from_env()
        .map(|cfg| Arc::new(codeplan_identity::oidc::OidcCtx::new(cfg)));
    let state = codeplan_identity::api::AuthState { auth, oidc, rbac };

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
