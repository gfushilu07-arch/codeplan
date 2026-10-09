//! HTTP 层:认证路由、当前用户提取器、OpenAPI 片段。

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::{Json, Router};
use codeplan_common::error::AppError;
use codeplan_common::response::ApiResponse;
use codeplan_kernel::identity::CurrentUser;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{OpenApi, ToSchema};

use crate::DOMAIN_CODE;
use crate::service::AuthService;

/// api 层共享状态。
#[derive(Clone)]
pub struct AuthState {
    /// 认证服务。
    pub auth: Arc<AuthService>,
}

/// 身份域路由(挂载到 `/api`)。
pub fn routes() -> Router<AuthState> {
    Router::new()
        .route("/api/auth/login", axum::routing::post(login))
        .route("/api/auth/logout", axum::routing::post(logout))
        .route("/api/me", axum::routing::get(me))
}

/// 登录请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    /// 登录名。
    pub username: String,
    /// 密码。
    pub password: String,
}

/// 登录响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponse {
    /// JWT(随请求 `Authorization: Bearer <token>` 携带)。
    pub token: String,
    /// 过期时间(RFC 3339)。
    pub expires_at: String,
    /// 用户 ID。
    pub user_id: String,
    /// 显示名。
    pub display_name: String,
    /// 是否平台管理员。
    pub is_platform_admin: bool,
}

/// 当前用户信息。
#[derive(Debug, Serialize, ToSchema)]
pub struct MeResponse {
    /// 用户 ID。
    pub id: String,
    /// 显示名。
    pub display_name: String,
    /// 邮箱。
    pub email: String,
    /// 是否平台管理员。
    pub is_platform_admin: bool,
}

/// 用户名 + 密码登录。
#[utoipa::path(
    post,
    path = "/api/auth/login",
    request_body = LoginRequest,
    responses((status = 200, body = LoginResponse), (status = 401, body = LoginResponse))
)]
async fn login(
    State(state): State<AuthState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<ApiResponse<LoginResponse>>, AppError> {
    let session = state.auth.login(&req.username, &req.password).await?;
    Ok(Json(ApiResponse::ok(LoginResponse {
        token: session.token,
        expires_at: chrono::DateTime::to_rfc3339(&session.expires_at),
        user_id: session.user_id.as_uuid().to_string(),
        display_name: session.display_name,
        is_platform_admin: session.is_platform_admin,
    })))
}

/// 登出(无状态 JWT:由客户端丢弃令牌;令牌吊销表随企业特性引入)。
#[utoipa::path(post, path = "/api/auth/logout", responses((status = 200)))]
async fn logout() -> Json<ApiResponse<()>> {
    Json(ApiResponse {
        code: "OK".to_owned(),
        message: "已登出".to_owned(),
        data: None,
    })
}

/// 当前用户信息。
#[utoipa::path(
    get,
    path = "/api/me",
    responses((status = 200, body = MeResponse), (status = 401, body = MeResponse))
)]
async fn me(user: CurrentUser) -> Json<ApiResponse<MeResponse>> {
    Json(ApiResponse::ok(MeResponse {
        id: user.id.as_uuid().to_string(),
        display_name: user.display_name,
        email: user.email,
        is_platform_admin: user.is_platform_admin,
    }))
}

/// 从 `Authorization: Bearer` 提取并校验当前用户(认证中间件)。
impl FromRequestParts<AuthState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AuthState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| AppError::unauthenticated(DOMAIN_CODE))?;
        state.auth.verify_token(token).await
    }
}

/// 身份域 OpenAPI 片段(供 server 聚合)。
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(login, logout, me),
    components(schemas(LoginRequest, LoginResponse, MeResponse))
)]
pub struct IdentityApiDoc;

/// 返回身份域 OpenAPI 片段。
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    IdentityApiDoc::openapi()
}
