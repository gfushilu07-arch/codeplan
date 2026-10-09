//! codeplan 组合根:装配所有业务域、基础设施与插件(L4,唯一二进制)。
//!
//! 当前为阶段 0 骨架:仅健康检查与 OpenAPI 契约端点,随批次任务逐步装配各域。

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

async fn openapi_json() -> Json<serde_json::Value> {
    Json(
        serde_json::to_value(ApiDoc::openapi()).expect("OpenAPI 文档序列化失败"),
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/docs/openapi.json", get(openapi_json));

    let addr = std::env::var("CODEPLAN_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_owned());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("codeplan-server 监听 http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
