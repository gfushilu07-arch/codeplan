//! HTTP 层:认证路由、当前用户提取器、OpenAPI 片段。

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::{Json, Router};
use codeplan_common::error::AppError;
use codeplan_common::response::ApiResponse;
use codeplan_kernel::identity::CurrentUser;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, OpenApi, ToSchema};

use crate::DOMAIN_CODE;
use crate::service::AuthService;

/// api 层共享状态。
#[derive(Clone)]
pub struct AuthState {
    /// 认证服务。
    pub auth: Arc<AuthService>,
    /// OIDC 上下文(未启用为 None)。
    pub oidc: Option<Arc<crate::oidc::OidcCtx>>,
    /// RBAC 服务。
    pub rbac: Arc<crate::rbac::RbacService>,
    /// LDAP 目录(未启用为 None)。
    pub ldap: Option<Arc<dyn crate::ldap::LdapDirectory>>,
    /// 审计查询(PG 实现;记录写入经服务内嵌槽)。
    pub audit: Arc<crate::infra::PgAuditSink>,
    /// 事件总线(域发布领域事件用;T-111 起接入业务)。
    pub event_bus: Arc<dyn codeplan_kernel::events::EventBus>,
}

/// 身份域路由(挂载到 `/api`)。
pub fn routes() -> Router<AuthState> {
    Router::new()
        .route("/api/auth/login", axum::routing::post(login))
        .route("/api/auth/login/totp", axum::routing::post(login_totp))
        .route("/api/auth/totp/setup", axum::routing::post(totp_setup))
        .route("/api/auth/totp/enable", axum::routing::post(totp_enable))
        .route("/api/auth/totp/disable", axum::routing::post(totp_disable))
        .route("/api/auth/logout", axum::routing::post(logout))
        .route("/api/me", axum::routing::get(me))
        .route("/api/rbac/grant", axum::routing::post(rbac_grant))
        .route("/api/rbac/revoke", axum::routing::post(rbac_revoke))
        .route("/api/rbac/members", axum::routing::get(rbac_members))
        .route("/api/rbac/my-role", axum::routing::get(rbac_my_role))
        .route("/api/admin/ldap/sync", axum::routing::post(ldap_sync))
        .route("/api/admin/audit", axum::routing::get(audit_recent))
}

/// 登录请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    /// 登录名。
    pub username: String,
    /// 密码。
    pub password: String,
}

/// 登录响应:直接发会话,或要求 TOTP 二次验证。
#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponse {
    /// 是否需要 TOTP 二次验证(为 true 时凭 `ticket` 调 `/api/auth/login/totp`)。
    pub requires_totp: bool,
    /// JWT(仅 `requires_totp = false` 时存在)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// 过期时间(仅 `requires_totp = false` 时存在)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    /// TOTP 二次验证票据(仅 `requires_totp = true` 时存在)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    /// 用户 ID(两种情形都有)。
    pub user_id: String,
    /// 显示名。
    pub display_name: String,
    /// 是否平台管理员。
    pub is_platform_admin: bool,
}

/// TOTP 二次验证请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct TotpLoginRequest {
    /// 登录返回的 MFA 票据。
    pub ticket: String,
    /// 6 位验证码。
    pub code: String,
}

/// TOTP setup 响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct TotpSetupResponse {
    /// base32 密钥(手工录入用)。
    pub secret: String,
    /// otpauth:// URL(扫码用)。
    pub otpauth_url: String,
}

/// TOTP 验证码请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct TotpCodeRequest {
    /// 6 位验证码。
    pub code: String,
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
    let result = state.auth.login(&req.username, &req.password).await?;
    let user_id = match &result {
        crate::service::LoginResult::Done(s) => s.user_id.as_uuid().to_string(),
        crate::service::LoginResult::TotpRequired { .. } => String::new(),
    };
    let display_name = match &result {
        crate::service::LoginResult::Done(s) => s.display_name.clone(),
        crate::service::LoginResult::TotpRequired { .. } => String::new(),
    };
    let is_admin = match &result {
        crate::service::LoginResult::Done(s) => s.is_platform_admin,
        crate::service::LoginResult::TotpRequired { .. } => false,
    };
    Ok(Json(ApiResponse::ok(match result {
        crate::service::LoginResult::Done(s) => LoginResponse {
            requires_totp: false,
            token: Some(s.token),
            expires_at: Some(chrono::DateTime::to_rfc3339(&s.expires_at)),
            ticket: None,
            user_id,
            display_name,
            is_platform_admin: is_admin,
        },
        crate::service::LoginResult::TotpRequired { ticket } => LoginResponse {
            requires_totp: true,
            token: None,
            expires_at: None,
            ticket: Some(ticket),
            user_id,
            display_name,
            is_platform_admin: is_admin,
        },
    })))
}

/// TOTP 二次验证换取会话。
#[utoipa::path(
    post,
    path = "/api/auth/login/totp",
    request_body = TotpLoginRequest,
    responses((status = 200, body = LoginResponse), (status = 401, body = LoginResponse))
)]
async fn login_totp(
    State(state): State<AuthState>,
    Json(req): Json<TotpLoginRequest>,
) -> Result<Json<ApiResponse<LoginResponse>>, AppError> {
    let session = state.auth.verify_totp_login(&req.ticket, &req.code).await?;
    Ok(Json(ApiResponse::ok(LoginResponse {
        requires_totp: false,
        token: Some(session.token),
        expires_at: Some(chrono::DateTime::to_rfc3339(&session.expires_at)),
        ticket: None,
        user_id: session.user_id.as_uuid().to_string(),
        display_name: session.display_name,
        is_platform_admin: session.is_platform_admin,
    })))
}

/// 生成 TOTP 密钥(待确认;enable 前登录不受影响)。
#[utoipa::path(
    post,
    path = "/api/auth/totp/setup",
    responses((status = 200, body = TotpSetupResponse))
)]
async fn totp_setup(
    user: CurrentUser,
    State(state): State<AuthState>,
) -> Result<Json<ApiResponse<TotpSetupResponse>>, AppError> {
    let (secret, url) = state.auth.totp_setup(user.id, &user.email).await?;
    Ok(Json(ApiResponse::ok(TotpSetupResponse {
        secret,
        otpauth_url: url,
    })))
}

/// 确认启用 TOTP。
#[utoipa::path(post, path = "/api/auth/totp/enable", request_body = TotpCodeRequest, responses((status = 200)))]
async fn totp_enable(
    user: CurrentUser,
    State(state): State<AuthState>,
    Json(req): Json<TotpCodeRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.auth.totp_enable(user.id, &req.code).await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "TOTP 已启用".to_owned(),
        data: None,
    }))
}

/// 关闭 TOTP。
#[utoipa::path(post, path = "/api/auth/totp/disable", request_body = TotpCodeRequest, responses((status = 200)))]
async fn totp_disable(
    user: CurrentUser,
    State(state): State<AuthState>,
    Json(req): Json<TotpCodeRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    state.auth.totp_disable(user.id, &req.code).await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "TOTP 已关闭".to_owned(),
        data: None,
    }))
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
    paths(login, login_totp, logout, me, totp_setup, totp_enable, totp_disable),
    components(schemas(
        LoginRequest,
        LoginResponse,
        TotpLoginRequest,
        TotpSetupResponse,
        TotpCodeRequest,
        MeResponse
    ))
)]
pub struct IdentityApiDoc;

/// 返回身份域 OpenAPI 片段。
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    IdentityApiDoc::openapi()
}

/// 授予成员角色请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct RbacGrantRequest {
    /// 资源种类:project/repo/app。
    pub resource_kind: String,
    /// 资源 ID。
    pub resource_id: String,
    /// 目标用户登录名。
    pub username: String,
    /// 角色:read/write/admin。
    pub role: String,
}

/// 撤销成员角色请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct RbacRevokeRequest {
    /// 资源种类。
    pub resource_kind: String,
    /// 资源 ID。
    pub resource_id: String,
    /// 目标用户登录名。
    pub username: String,
}

/// 成员条目。
#[derive(Debug, Serialize, ToSchema)]
pub struct RbacMemberResponse {
    /// 用户 ID。
    pub user_id: String,
    /// 登录名。
    pub username: String,
    /// 显示名。
    pub display_name: String,
    /// 角色。
    pub role: String,
}

/// 我的角色。
#[derive(Debug, Serialize, ToSchema)]
pub struct MyRoleResponse {
    /// 角色(read/write/admin),无则 null。
    pub role: Option<String>,
}

fn parse_role(role: &str) -> Result<crate::domain::ResourceRole, AppError> {
    crate::domain::ResourceRole::from_str_opt(role)
        .ok_or_else(|| AppError::invalid(DOMAIN_CODE, format!("非法角色:{role}")))
}

/// 管理员授予/更新成员角色。
#[utoipa::path(
    post,
    path = "/api/rbac/grant",
    request_body = RbacGrantRequest,
    responses((status = 200), (status = 403))
)]
async fn rbac_grant(
    user: CurrentUser,
    State(state): State<AuthState>,
    Json(req): Json<RbacGrantRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let resource = crate::rbac::ResourceLocator::parse(&req.resource_kind, &req.resource_id)?;
    let role = parse_role(&req.role)?;
    state
        .rbac
        .grant(&user, resource, &req.username, role)
        .await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "已授予".to_owned(),
        data: None,
    }))
}

/// 管理员撤销成员角色。
#[utoipa::path(
    post,
    path = "/api/rbac/revoke",
    request_body = RbacRevokeRequest,
    responses((status = 200), (status = 403))
)]
async fn rbac_revoke(
    user: CurrentUser,
    State(state): State<AuthState>,
    Json(req): Json<RbacRevokeRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let resource = crate::rbac::ResourceLocator::parse(&req.resource_kind, &req.resource_id)?;
    state.rbac.revoke(&user, resource, &req.username).await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "已撤销".to_owned(),
        data: None,
    }))
}

/// 成员列表查询参数。
#[derive(Debug, Deserialize, IntoParams)]
pub struct RbacResourceQuery {
    /// 资源种类。
    pub kind: String,
    /// 资源 ID。
    pub id: String,
}

/// 成员列表。
#[utoipa::path(
    get,
    path = "/api/rbac/members",
    params(RbacResourceQuery),
    responses((status = 200, body = RbacMemberResponse))
)]
async fn rbac_members(
    user: CurrentUser,
    State(state): State<AuthState>,
    axum::extract::Query(q): axum::extract::Query<RbacResourceQuery>,
) -> Result<Json<ApiResponse<Vec<RbacMemberResponse>>>, AppError> {
    let resource = crate::rbac::ResourceLocator::parse(&q.kind, &q.id)?;
    let members = state.rbac.members(&user, resource).await?;
    Ok(Json(ApiResponse::ok(
        members
            .into_iter()
            .map(|m| RbacMemberResponse {
                user_id: m.user_id.as_uuid().to_string(),
                username: m.username,
                display_name: m.display_name,
                role: m.role.as_str().to_owned(),
            })
            .collect(),
    )))
}

/// 查询自己在资源上的角色。
#[utoipa::path(
    get,
    path = "/api/rbac/my-role",
    params(RbacResourceQuery),
    responses((status = 200, body = MyRoleResponse))
)]
async fn rbac_my_role(
    user: CurrentUser,
    State(state): State<AuthState>,
    axum::extract::Query(q): axum::extract::Query<RbacResourceQuery>,
) -> Result<Json<ApiResponse<MyRoleResponse>>, AppError> {
    let resource = crate::rbac::ResourceLocator::parse(&q.kind, &q.id)?;
    let role = state.rbac.my_role(&user, resource).await?;
    Ok(Json(ApiResponse::ok(MyRoleResponse {
        role: role.map(|r| r.as_str().to_owned()),
    })))
}

/// LDAP 手动同步响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct LdapSyncResponse {
    /// 同步条数。
    pub synced: usize,
}

/// 手动触发 LDAP 同步(平台管理员)。
#[utoipa::path(post, path = "/api/admin/ldap/sync", responses((status = 200, body = LdapSyncResponse)))]
async fn ldap_sync(
    user: Result<CurrentUser, AppError>,
    State(state): State<AuthState>,
) -> Result<Json<ApiResponse<LdapSyncResponse>>, AppError> {
    let user = user?;
    if !user.is_platform_admin {
        return Err(AppError::forbidden(DOMAIN_CODE, "仅平台管理员可触发同步"));
    }
    let synced = state.auth.sync_ldap_users().await?;
    Ok(Json(ApiResponse::ok(LdapSyncResponse { synced })))
}

/// 审计记录。
#[derive(Debug, Serialize, ToSchema)]
pub struct AuditRecordResponse {
    /// 记录 ID。
    pub id: String,
    /// 操作者(可为空)。
    pub actor: Option<String>,
    /// 动作。
    pub action: String,
    /// 资源种类。
    pub resource_kind: Option<String>,
    /// 资源 ID。
    pub resource_id: Option<String>,
    /// 附加细节。
    pub detail: serde_json::Value,
    /// 时间(RFC 3339)。
    pub at: String,
}

/// 审计查询参数。
#[derive(Debug, Deserialize, IntoParams)]
pub struct AuditQuery {
    /// 返回条数上限(1~500,默认 100)。
    pub limit: Option<i64>,
}

/// 最近审计记录(平台管理员)。
#[utoipa::path(
    get,
    path = "/api/admin/audit",
    params(AuditQuery),
    responses((status = 200, body = AuditRecordResponse))
)]
async fn audit_recent(
    user: Result<CurrentUser, AppError>,
    State(state): State<AuthState>,
    axum::extract::Query(q): axum::extract::Query<AuditQuery>,
) -> Result<Json<ApiResponse<Vec<AuditRecordResponse>>>, AppError> {
    let user = user?;
    if !user.is_platform_admin {
        return Err(AppError::forbidden(DOMAIN_CODE, "仅平台管理员可查询审计"));
    }
    let records = state.audit.recent(q.limit.unwrap_or(100)).await?;
    Ok(Json(ApiResponse::ok(
        records
            .into_iter()
            .map(|r| AuditRecordResponse {
                id: r.id.to_string(),
                actor: r.actor.map(|u| u.as_uuid().to_string()),
                action: r.action,
                resource_kind: match &r.resource {
                    codeplan_kernel::authz::ResourceRef::System => Some("system".to_owned()),
                    codeplan_kernel::authz::ResourceRef::Project(_) => Some("project".to_owned()),
                    codeplan_kernel::authz::ResourceRef::Repo(_) => Some("repo".to_owned()),
                    codeplan_kernel::authz::ResourceRef::App(_) => Some("app".to_owned()),
                },
                resource_id: match &r.resource {
                    codeplan_kernel::authz::ResourceRef::Project(id) => {
                        Some(id.as_uuid().to_string())
                    }
                    codeplan_kernel::authz::ResourceRef::Repo(id) => Some(id.as_uuid().to_string()),
                    codeplan_kernel::authz::ResourceRef::App(id) => Some(id.as_uuid().to_string()),
                    codeplan_kernel::authz::ResourceRef::System => None,
                },
                detail: r.detail,
                at: chrono::DateTime::to_rfc3339(&r.at),
            })
            .collect(),
    )))
}
