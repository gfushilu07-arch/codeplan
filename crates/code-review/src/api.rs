//! PR API:axum 路由 + DTO。

use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use codeplan_common::error::AppError;
use codeplan_kernel::authn::TokenAuthenticator;
use codeplan_kernel::git::MergeMethod;
use codeplan_kernel::identity::CurrentUser;
use serde::{Deserialize, Serialize};

use crate::domain::{PrEdit, PrState, PullRequest};
use crate::service::{DOMAIN_CODE, PullRequestService};

/// API 状态。
#[derive(Clone)]
pub struct PrApiState {
    /// PR 服务。
    pub svc: std::sync::Arc<PullRequestService>,
    /// 评审流服务(T-204)。
    pub review: std::sync::Arc<crate::review_service::ReviewService>,
    /// 令牌认证器。
    pub authn: std::sync::Arc<dyn TokenAuthenticator>,
    /// 推送门禁(T-205,server 经此装配到 git 传输)。
    pub push_guard: std::sync::Arc<dyn codeplan_kernel::git::PushGuard>,
}

impl PrApiState {
    /// 供 server 装配 transport。
    #[must_use]
    pub fn push_guard(&self) -> std::sync::Arc<dyn codeplan_kernel::git::PushGuard> {
        std::sync::Arc::clone(&self.push_guard)
    }
}

/// 路由:`/api/repos/{owner}/{key}/pulls`。
pub fn routes(state: PrApiState) -> Router<PrApiState> {
    Router::new()
        .route(
            "/api/repos/{owner}/{key}/pulls",
            post(create_pr).get(list_prs),
        )
        .route(
            "/api/repos/{owner}/{key}/pulls/{number}",
            get(get_pr).put(edit_pr),
        )
        .route("/api/repos/{owner}/{key}/pulls/{number}/diff", get(pr_diff))
        .route(
            "/api/repos/{owner}/{key}/pulls/{number}/commits",
            get(pr_commits),
        )
        .route(
            "/api/repos/{owner}/{key}/pulls/{number}/merge",
            post(merge_pr),
        )
        .route(
            "/api/repos/{owner}/{key}/pulls/{number}/close",
            post(close_pr),
        )
        .route(
            "/api/repos/{owner}/{key}/pulls/{number}/reopen",
            post(reopen_pr),
        )
        .merge(crate::review_api::review_routes())
        .layer(middleware::from_fn_with_state(state, auth_mw))
}

/// 认证中间件(Bearer JWT → CurrentUser)。
async fn auth_mw(
    State(st): State<PrApiState>,
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
struct CreatePrRequest {
    /// 标题。
    pub title: String,
    /// 描述。
    #[serde(default)]
    pub body: String,
    /// 源分支。
    pub source_branch: String,
    /// 目标分支(缺省 = 默认分支)。
    pub target_branch: Option<String>,
    /// 是否草稿。
    #[serde(default)]
    pub draft: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
struct PrResponse {
    /// PR ID。
    pub id: String,
    /// 仓库内序号。
    pub number: i64,
    /// 标题。
    pub title: String,
    /// 描述。
    pub body: String,
    /// 源分支。
    pub source_branch: String,
    /// 目标分支。
    pub target_branch: String,
    /// 创建人。
    pub author: String,
    /// 状态。
    pub state: String,
    /// 是否草稿。
    pub draft: bool,
    /// 合并方式(合并后)。
    pub merge_method: Option<String>,
    /// 合并提交(合并后)。
    pub merge_commit: Option<String>,
    /// 创建时间(RFC 3339)。
    pub created_at: String,
    /// 更新时间(RFC 3339)。
    pub updated_at: String,
}

fn pr_response(pr: &PullRequest) -> PrResponse {
    PrResponse {
        id: pr.id.as_uuid().to_string(),
        number: pr.number,
        title: pr.title.clone(),
        body: pr.body.clone(),
        source_branch: pr.source_branch.clone(),
        target_branch: pr.target_branch.clone(),
        author: pr.author.as_uuid().to_string(),
        state: pr.state.as_str().to_owned(),
        draft: pr.is_draft,
        merge_method: pr.merge_method.clone(),
        merge_commit: pr.merge_commit.clone(),
        created_at: pr.created_at.to_rfc3339(),
        updated_at: pr.updated_at.to_rfc3339(),
    }
}

async fn create_pr(
    State(st): State<PrApiState>,
    Path((owner, key)): Path<(String, String)>,
    req: axum::Extension<CurrentUser>,
    Json(body): Json<CreatePrRequest>,
) -> Result<Response, AppError> {
    let pr = st
        .svc
        .create(
            &req.0,
            &format!("{owner}/{key}"),
            &body.title,
            &body.body,
            &body.source_branch,
            body.target_branch.as_deref(),
            body.draft,
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"code": "OK", "data": pr_response(&pr)})),
    )
        .into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct ListPrQuery {
    /// 状态过滤(open/merged/closed)。
    pub state: Option<String>,
}

async fn list_prs(
    State(st): State<PrApiState>,
    Path((owner, key)): Path<(String, String)>,
    req: axum::Extension<CurrentUser>,
    Query(q): Query<ListPrQuery>,
) -> Result<Response, AppError> {
    let state = match q.state.as_deref() {
        None | Some("") => None,
        Some(s) => Some(
            PrState::from_str_opt(s)
                .ok_or_else(|| AppError::invalid(DOMAIN_CODE, "state 取值非法"))?,
        ),
    };
    let prs = st
        .svc
        .list(&req.0, &format!("{owner}/{key}"), state)
        .await?;
    Ok(Json(serde_json::json!({
        "code": "OK",
        "data": prs.iter().map(pr_response).collect::<Vec<_>>(),
    }))
    .into_response())
}

async fn get_pr(
    State(st): State<PrApiState>,
    Path((owner, key, number)): Path<(String, String, i64)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let pr = st
        .svc
        .get(&req.0, &format!("{owner}/{key}"), number)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": pr_response(&pr)})).into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct EditPrRequest {
    /// 标题。
    pub title: Option<String>,
    /// 描述。
    pub body: Option<String>,
    /// 草稿标记。
    pub draft: Option<bool>,
}

async fn edit_pr(
    State(st): State<PrApiState>,
    Path((owner, key, number)): Path<(String, String, i64)>,
    req: axum::Extension<CurrentUser>,
    Json(body): Json<EditPrRequest>,
) -> Result<Response, AppError> {
    let pr = st
        .svc
        .edit(
            &req.0,
            &format!("{owner}/{key}"),
            number,
            &PrEdit {
                title: body.title,
                body: body.body,
                is_draft: body.draft,
            },
        )
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": pr_response(&pr)})).into_response())
}

async fn pr_diff(
    State(st): State<PrApiState>,
    Path((owner, key, number)): Path<(String, String, i64)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let patch = st
        .svc
        .diff(&req.0, &format!("{owner}/{key}"), number)
        .await?;
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], patch).into_response())
}

async fn pr_commits(
    State(st): State<PrApiState>,
    Path((owner, key, number)): Path<(String, String, i64)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let commits = st
        .svc
        .commits(&req.0, &format!("{owner}/{key}"), number)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": commits})).into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct MergePrRequest {
    /// 合并方式:merge / squash / rebase。
    #[serde(default = "default_method")]
    pub method: String,
    /// 提交信息标题(缺省 = PR 标题)。
    pub message_title: Option<String>,
    /// 提交信息正文(缺省 = PR 描述)。
    pub message_body: Option<String>,
}

fn default_method() -> String {
    "merge".to_owned()
}

async fn merge_pr(
    State(st): State<PrApiState>,
    Path((owner, key, number)): Path<(String, String, i64)>,
    req: axum::Extension<CurrentUser>,
    Json(body): Json<MergePrRequest>,
) -> Result<Response, AppError> {
    let method = match body.method.as_str() {
        "merge" => MergeMethod::Merge,
        "squash" => MergeMethod::Squash,
        "rebase" => MergeMethod::Rebase,
        _ => return Err(AppError::invalid(DOMAIN_CODE, "method 取值非法")),
    };
    let pr = st
        .svc
        .merge(
            &req.0,
            &format!("{owner}/{key}"),
            number,
            method,
            body.message_title,
            body.message_body,
        )
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": pr_response(&pr)})).into_response())
}

async fn close_pr(
    State(st): State<PrApiState>,
    Path((owner, key, number)): Path<(String, String, i64)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let pr = st
        .svc
        .transition(&req.0, &format!("{owner}/{key}"), number, PrState::Closed)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": pr_response(&pr)})).into_response())
}

async fn reopen_pr(
    State(st): State<PrApiState>,
    Path((owner, key, number)): Path<(String, String, i64)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let pr = st
        .svc
        .transition(&req.0, &format!("{owner}/{key}"), number, PrState::Open)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": pr_response(&pr)})).into_response())
}
