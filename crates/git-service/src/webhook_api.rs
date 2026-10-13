//! Webhook 订阅 API(T-209)。

use axum::extract::{Path, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use codeplan_common::error::AppError;
use codeplan_kernel::authn::TokenAuthenticator;
use codeplan_kernel::identity::CurrentUser;
use serde::Deserialize;

use crate::DOMAIN_CODE;
use crate::webhook_service::WebhookService;

/// API 状态。
#[derive(Clone)]
pub struct WebhookState {
    /// Webhook 服务。
    pub svc: std::sync::Arc<WebhookService>,
    /// 令牌认证器。
    pub authn: std::sync::Arc<dyn TokenAuthenticator>,
}

/// 路由。
pub fn routes(state: WebhookState) -> Router<WebhookState> {
    Router::new()
        .route(
            "/api/repos/{owner}/{key}/hooks",
            post(create_hook).get(list_hooks),
        )
        .route(
            "/api/repos/{owner}/{key}/hooks/{id}",
            axum::routing::delete(delete_hook),
        )
        .route("/api/repos/{owner}/{key}/hooks/{id}/test", post(test_hook))
        .route(
            "/api/repos/{owner}/{key}/hooks/{id}/deliveries",
            get(list_deliveries),
        )
        .layer(middleware::from_fn_with_state(state, auth_mw))
}

async fn auth_mw(
    State(st): State<WebhookState>,
    mut req: axum::extract::Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::unauthenticated(DOMAIN_CODE))?;
    let user = st
        .authn
        .verify(token)
        .await
        .map_err(|_| AppError::unauthenticated(DOMAIN_CODE))?;
    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

fn full_name(owner: &str, key: &str) -> String {
    format!("{owner}/{key}")
}

fn hook_json(h: &crate::webhook::Webhook) -> serde_json::Value {
    serde_json::json!({
        "id": h.id.as_uuid(),
        "url": h.url,
        "secret_masked": format!("{}…", &h.secret[..h.secret.len().min(4)]),
        "events": h.events,
        "active": h.active,
        "created_at": h.created_at.to_rfc3339(),
    })
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct CreateHookRequest {
    /// 回调 URL(http/https)。
    pub url: String,
    /// HMAC 密钥(缺省自动生成)。
    pub secret: Option<String>,
    /// 订阅事件类型(空 = 全部)。
    #[serde(default)]
    pub events: Vec<String>,
}

async fn create_hook(
    State(st): State<WebhookState>,
    Path((owner, key)): Path<(String, String)>,
    req: axum::Extension<CurrentUser>,
    Json(body): Json<CreateHookRequest>,
) -> Result<Response, AppError> {
    let hook = st
        .svc
        .create(
            &req.0,
            &full_name(&owner, &key),
            &body.url,
            body.secret.as_deref(),
            body.events,
        )
        .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({"code": "OK", "data": hook_json(&hook)})),
    )
        .into_response())
}

async fn list_hooks(
    State(st): State<WebhookState>,
    Path((owner, key)): Path<(String, String)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let hooks = st.svc.list(&req.0, &full_name(&owner, &key)).await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": hooks
        .iter()
        .map(hook_json)
        .collect::<Vec<_>>()}))
    .into_response())
}

async fn delete_hook(
    State(st): State<WebhookState>,
    Path((owner, key, id)): Path<(String, String, uuid::Uuid)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    st.svc
        .delete(
            &req.0,
            &full_name(&owner, &key),
            crate::webhook::WebhookId::from_uuid(id),
        )
        .await?;
    Ok(Json(serde_json::json!({"code": "OK"})).into_response())
}

async fn test_hook(
    State(st): State<WebhookState>,
    Path((owner, key, id)): Path<(String, String, uuid::Uuid)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let full = full_name(&owner, &key);
    let hook_id = crate::webhook::WebhookId::from_uuid(id);
    let hook = st.svc.find_hook_checked(&req.0, &full, hook_id).await?;
    let envelope = crate::webhook_service::ping_envelope(hook.repo_id.as_uuid());
    let record = st.svc.deliver(&hook, "ping", &envelope).await;
    Ok(Json(serde_json::json!({"code": "OK", "data": {
        "ok": record.ok,
        "attempts": record.attempts,
        "status_code": record.status_code,
        "error": record.error,
    }}))
    .into_response())
}

async fn list_deliveries(
    State(st): State<WebhookState>,
    Path((owner, key, id)): Path<(String, String, uuid::Uuid)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let records = st
        .svc
        .deliveries(
            &req.0,
            &full_name(&owner, &key),
            crate::webhook::WebhookId::from_uuid(id),
            50,
        )
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": records
        .iter()
        .map(|d| serde_json::json!({
            "event": d.event_type,
            "ok": d.ok,
            "status_code": d.status_code,
            "attempts": d.attempts,
            "error": d.error,
            "created_at": d.created_at.to_rfc3339(),
        }))
        .collect::<Vec<_>>()}))
    .into_response())
}
