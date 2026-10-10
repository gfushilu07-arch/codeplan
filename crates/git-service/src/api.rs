//! HTTP 层:仓库生命周期路由。认证经 kernel::TokenAuthenticator 中间件。

use axum::extract::{Path, State};
use axum::http::header::AUTHORIZATION;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Extension, Json, Router, middleware};
use codeplan_common::error::AppError;
use codeplan_common::response::ApiResponse;
use codeplan_kernel::authn::TokenAuthenticator;
use codeplan_kernel::identity::CurrentUser;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use crate::DOMAIN_CODE;
use crate::domain::Visibility;
use crate::service::GitService;

/// api 层状态。
#[derive(Clone)]
pub struct GitState {
    /// Git 域服务。
    pub svc: Arc<GitService>,
    /// 令牌认证器。
    pub authn: Arc<dyn TokenAuthenticator>,
    /// 仓库根(GIT_PROJECT_ROOT,http-backend 用)。
    pub repo_root: String,
    /// 内部钩子密钥(post-receive 回调校验)。
    pub hook_secret: String,
}

/// 路由。
pub fn routes(state: GitState) -> Router<GitState> {
    Router::new()
        .route("/api/repos", post(create_repository).get(list_repositories))
        .route(
            "/api/repos/{owner}/{key}",
            get(get_repository)
                .delete(delete_repository)
                .patch(patch_repository),
        )
        .merge(crate::browse_api::browse_routes())
        .layer(middleware::from_fn_with_state(state, auth_mw))
}

/// 认证中间件。
async fn auth_mw(
    State(st): State<GitState>,
    mut req: axum::extract::Request,
    next: middleware::Next,
) -> Result<Response, AppError> {
    let token = req
        .headers()
        .get(AUTHORIZATION)
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

/// 仓库响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct RepoResponse {
    /// 仓库 ID。
    pub id: String,
    /// 全名(owner/key)。
    pub full_name: String,
    /// 名称。
    pub name: String,
    /// 描述。
    pub description: String,
    /// 可见性。
    pub visibility: String,
    /// 默认分支。
    pub default_branch: String,
    /// 是否已归档。
    pub archived: bool,
    /// 创建时间(RFC 3339)。
    pub created_at: String,
}

/// 创建仓库请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateRepoRequest {
    /// 仓库键(小写字母/数字/连字符)。
    pub key: String,
    /// 名称。
    pub name: String,
    /// 描述。
    #[serde(default)]
    pub description: String,
    /// 可见性(private/public,默认 private)。
    pub visibility: Option<String>,
    /// 默认分支(默认 main)。
    pub default_branch: Option<String>,
    /// 关联项目 ID(可选)。
    pub project_id: Option<String>,
}

/// 更新仓库请求(PATCH)。
#[derive(Debug, Deserialize, ToSchema)]
pub struct PatchRepoRequest {
    /// 设置默认分支。
    pub default_branch: Option<String>,
    /// 归档(true)/恢复(false)。
    pub archived: Option<bool>,
}

fn repo_response(repo: &crate::domain::Repository) -> RepoResponse {
    RepoResponse {
        id: repo.id.as_uuid().to_string(),
        full_name: repo.full_name.clone(),
        name: repo.name.clone(),
        description: repo.description.clone(),
        visibility: repo.visibility.as_str().to_owned(),
        default_branch: repo.default_branch.clone(),
        archived: repo.archived,
        created_at: chrono::DateTime::to_rfc3339(&repo.created_at),
    }
}

async fn create_repository(
    Extension(user): Extension<CurrentUser>,
    State(st): State<GitState>,
    Json(req): Json<CreateRepoRequest>,
) -> Result<Json<ApiResponse<RepoResponse>>, AppError> {
    let visibility = req
        .visibility
        .as_deref()
        .and_then(Visibility::from_str_opt)
        .unwrap_or(Visibility::Private);
    // 所有者键 = 当前用户名(简化:以登录名作为命名空间;企业域账号即工号)
    let owner_key = "admin"; // 占位:T-124 平台管理/用户中心引入后改为 actor 用户名
    let repo = st
        .svc
        .create_repository(
            &user,
            owner_key,
            &req.key,
            &req.name,
            &req.description,
            visibility,
            req.default_branch.as_deref().unwrap_or("main"),
            req.project_id
                .as_deref()
                .map(codeplan_kernel::ids::ProjectId::try_from)
                .transpose()
                .map_err(|_| AppError::invalid(DOMAIN_CODE, "非法项目 ID"))?,
        )
        .await?;
    Ok(Json(ApiResponse::ok(repo_response(&repo))))
}

async fn list_repositories(
    Extension(user): Extension<CurrentUser>,
    State(st): State<GitState>,
) -> Result<Json<ApiResponse<Vec<RepoResponse>>>, AppError> {
    let _ = user;
    let repos = st.svc.list_repositories().await?;
    Ok(Json(ApiResponse::ok(
        repos.iter().map(repo_response).collect(),
    )))
}

async fn get_repository(
    Extension(user): Extension<CurrentUser>,
    State(st): State<GitState>,
    Path((owner, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<RepoResponse>>, AppError> {
    let _ = user;
    let repo = st
        .svc
        .repository_by_full_name(&format!("{owner}/{key}"))
        .await?;
    Ok(Json(ApiResponse::ok(repo_response(&repo))))
}

async fn delete_repository(
    Extension(user): Extension<CurrentUser>,
    State(st): State<GitState>,
    Path((owner, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let repo = st
        .svc
        .repository_by_full_name(&format!("{owner}/{key}"))
        .await?;
    st.svc.set_archived(&user, &repo, true).await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "已归档".to_owned(),
        data: None,
    }))
}

async fn patch_repository(
    Extension(user): Extension<CurrentUser>,
    State(st): State<GitState>,
    Path((owner, key)): Path<(String, String)>,
    Json(req): Json<PatchRepoRequest>,
) -> Result<Json<ApiResponse<RepoResponse>>, AppError> {
    let repo = st
        .svc
        .repository_by_full_name(&format!("{owner}/{key}"))
        .await?;
    if let Some(branch) = &req.default_branch {
        st.svc.set_default_branch(&user, &repo, branch).await?;
    }
    if let Some(archived) = req.archived {
        st.svc.set_archived(&user, &repo, archived).await?;
    }
    let repo = st
        .svc
        .repository_by_full_name(&format!("{owner}/{key}"))
        .await?;
    Ok(Json(ApiResponse::ok(repo_response(&repo))))
}
