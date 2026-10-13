//! 提交签名验证 API(T-210)。

use axum::extract::{Path, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use codeplan_common::error::AppError;
use codeplan_kernel::authn::TokenAuthenticator;
use codeplan_kernel::identity::CurrentUser;
use serde::Deserialize;

use crate::DOMAIN_CODE;
use crate::signing::SigningService;

/// API 状态。
#[derive(Clone)]
pub struct SigningState {
    /// 签名验证服务。
    pub svc: std::sync::Arc<SigningService>,
    /// 令牌认证器。
    pub authn: std::sync::Arc<dyn TokenAuthenticator>,
}

/// 路由。
pub fn routes(state: SigningState) -> Router<SigningState> {
    Router::new()
        .route(
            "/api/repos/{owner}/{key}/signing/verify",
            post(verify_commit),
        )
        .layer(middleware::from_fn_with_state(state, auth_mw))
}

async fn auth_mw(
    State(st): State<SigningState>,
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

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct VerifyRequest {
    /// 提交 SHA。
    pub sha: String,
}

async fn verify_commit(
    State(st): State<SigningState>,
    Path((owner, key)): Path<(String, String)>,
    req: axum::Extension<CurrentUser>,
    Json(body): Json<VerifyRequest>,
) -> Result<Response, AppError> {
    let outcome = st
        .svc
        .verify_commit(&req.0, &format!("{owner}/{key}"), &body.sha)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": {
        "verified": outcome.verified,
        "method": outcome.method,
        "signer": outcome.signer.map(|u| u.as_uuid()),
        "fingerprint": outcome.fingerprint,
    }}))
    .into_response())
}
