//! 评审流 API(T-204):评论 / 评审 / 状态检查路由与 DTO。

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use codeplan_common::error::AppError;
use codeplan_kernel::identity::CurrentUser;
use serde::Deserialize;

use crate::review::{CheckState, CommentId, InlineAnchor, Side, Verdict};
use crate::review_service::ReviewService;
use crate::service::DOMAIN_CODE;

/// 评审流子路由(挂在主路由上,共用认证中间件)。
pub fn review_routes() -> Router<crate::api::PrApiState> {
    Router::new()
        .route(
            "/api/repos/{owner}/{key}/pulls/{number}/comments",
            post(create_comment).get(list_comments),
        )
        .route(
            "/api/repos/{owner}/{key}/pulls/{number}/comments/{comment_id}",
            axum::routing::delete(delete_comment),
        )
        .route(
            "/api/repos/{owner}/{key}/pulls/{number}/reviews",
            post(post_review).get(list_reviews),
        )
        .route(
            "/api/repos/{owner}/{key}/pulls/{number}/review-summary",
            get(review_summary),
        )
        .route(
            "/api/repos/{owner}/{key}/pulls/{number}/status",
            get(pr_status),
        )
        .route("/api/repos/{owner}/{key}/statuses", post(set_status))
        .route("/api/repos/{owner}/{key}/codeowners", get(codeowners_info))
        .route(
            "/api/repos/{owner}/{key}/pulls/{number}/reviewers",
            post(add_reviewer).get(list_reviewers),
        )
        .route(
            "/api/repos/{owner}/{key}/pulls/{number}/reviewers/{username}",
            axum::routing::delete(remove_reviewer),
        )
        .route(
            "/api/repos/{owner}/{key}/branch-protection",
            get(list_protections)
                .put(set_protection)
                .delete(delete_protection),
        )
        .route(
            "/api/repos/{owner}/{key}/commits/{sha}/statuses",
            get(commit_statuses),
        )
}

fn svc(st: &crate::api::PrApiState) -> &ReviewService {
    &st.review
}

fn full_name(owner: &str, key: &str) -> String {
    format!("{owner}/{key}")
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct CreateCommentRequest {
    /// 内容。
    pub body: String,
    /// 行内锚定文件(None = 对话流评论)。
    pub file_path: Option<String>,
    /// 行号。
    pub line: Option<i32>,
    /// 侧(LEFT/RIGHT,默认 RIGHT)。
    pub side: Option<String>,
}

async fn create_comment(
    State(st): State<crate::api::PrApiState>,
    Path((owner, key, number)): Path<(String, String, i64)>,
    req: axum::Extension<CurrentUser>,
    Json(body): Json<CreateCommentRequest>,
) -> Result<Response, AppError> {
    let anchor = match (&body.file_path, body.line) {
        (Some(file), line) => Some(InlineAnchor {
            file_path: file.clone(),
            line: line.unwrap_or(1),
            side: body
                .side
                .as_deref()
                .and_then(Side::from_str_opt)
                .unwrap_or(Side::Right),
        }),
        (None, _) => None,
    };
    let comment = svc(&st)
        .add_comment(&req.0, &full_name(&owner, &key), number, &body.body, anchor)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"code": "OK", "data": {
            "id": comment.id.as_uuid(),
            "author": comment.author.as_uuid(),
            "body": comment.body,
            "file_path": comment.file_path,
            "line": comment.line,
            "side": comment.side.map(|s| s.as_str()),
            "anchor_sha": comment.anchor_sha,
            "created_at": comment.created_at.to_rfc3339(),
        }})),
    )
        .into_response())
}

async fn list_comments(
    State(st): State<crate::api::PrApiState>,
    Path((owner, key, number)): Path<(String, String, i64)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let comments = svc(&st)
        .list_comments(&req.0, &full_name(&owner, &key), number)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": comments
        .iter()
        .map(|c| serde_json::json!({
            "id": c.id.as_uuid(),
            "author": c.author.as_uuid(),
            "body": c.body,
            "file_path": c.file_path,
            "line": c.line,
            "side": c.side.map(|s| s.as_str()),
            "anchor_sha": c.anchor_sha,
            "created_at": c.created_at.to_rfc3339(),
        }))
        .collect::<Vec<_>>()}))
    .into_response())
}

async fn delete_comment(
    State(st): State<crate::api::PrApiState>,
    Path((owner, key, number, comment_id)): Path<(String, String, i64, uuid::Uuid)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    svc(&st)
        .delete_comment(
            &req.0,
            &full_name(&owner, &key),
            number,
            CommentId::from_uuid(comment_id),
        )
        .await?;
    Ok(Json(serde_json::json!({"code": "OK"})).into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct PostReviewRequest {
    /// 结论:approved / changes_requested / comment。
    pub verdict: String,
    /// 说明。
    #[serde(default)]
    pub body: String,
}

async fn post_review(
    State(st): State<crate::api::PrApiState>,
    Path((owner, key, number)): Path<(String, String, i64)>,
    req: axum::Extension<CurrentUser>,
    Json(body): Json<PostReviewRequest>,
) -> Result<Response, AppError> {
    let verdict = Verdict::from_str_opt(&body.verdict)
        .ok_or_else(|| AppError::invalid(DOMAIN_CODE, "verdict 取值非法"))?;
    let review = svc(&st)
        .post_review(
            &req.0,
            &full_name(&owner, &key),
            number,
            verdict,
            &body.body,
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"code": "OK", "data": {
            "id": review.id.as_uuid(),
            "reviewer": review.reviewer.as_uuid(),
            "verdict": review.verdict.as_str(),
            "body": review.body,
            "head_sha": review.head_sha,
            "created_at": review.created_at.to_rfc3339(),
        }})),
    )
        .into_response())
}

async fn list_reviews(
    State(st): State<crate::api::PrApiState>,
    Path((owner, key, number)): Path<(String, String, i64)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let reviews = svc(&st)
        .list_reviews(&req.0, &full_name(&owner, &key), number)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": reviews
        .iter()
        .map(|r| serde_json::json!({
            "id": r.id.as_uuid(),
            "reviewer": r.reviewer.as_uuid(),
            "verdict": r.verdict.as_str(),
            "body": r.body,
            "head_sha": r.head_sha,
            "created_at": r.created_at.to_rfc3339(),
        }))
        .collect::<Vec<_>>()}))
    .into_response())
}

async fn review_summary(
    State(st): State<crate::api::PrApiState>,
    Path((owner, key, number)): Path<(String, String, i64)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let s = svc(&st)
        .review_summary(&req.0, &full_name(&owner, &key), number)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": {
        "approvals": s.approvals,
        "changes_requested": s.changes_requested,
        "combined": s.combined,
    }}))
    .into_response())
}

async fn pr_status(
    State(st): State<crate::api::PrApiState>,
    Path((owner, key, number)): Path<(String, String, i64)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let s = svc(&st)
        .pr_status(&req.0, &full_name(&owner, &key), number)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": {
        "combined": s.combined,
        "checks": s.checks.iter().map(|c| serde_json::json!({
            "context": c.context,
            "state": c.state.as_str(),
            "description": c.description,
            "target_url": c.target_url,
        })).collect::<Vec<_>>(),
    }}))
    .into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct SetStatusRequest {
    /// 提交 SHA。
    pub commit_sha: String,
    /// 检查上下文(如 ci/build)。
    pub context: String,
    /// 状态:success / failure / pending / error。
    pub state: String,
    /// 描述。
    #[serde(default)]
    pub description: String,
    /// 详情链接。
    pub target_url: Option<String>,
}

async fn set_status(
    State(st): State<crate::api::PrApiState>,
    Path((owner, key)): Path<(String, String)>,
    req: axum::Extension<CurrentUser>,
    Json(body): Json<SetStatusRequest>,
) -> Result<Response, AppError> {
    let state = CheckState::from_str_opt(&body.state)
        .ok_or_else(|| AppError::invalid(DOMAIN_CODE, "state 取值非法"))?;
    let repo = svc(&st)
        .set_status(
            &req.0,
            &full_name(&owner, &key),
            &body.commit_sha,
            &body.context,
            state,
            &body.description,
            body.target_url,
        )
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": {
        "repo": repo.full_name,
        "commit_sha": body.commit_sha,
        "context": body.context,
        "state": body.state,
    }}))
    .into_response())
}

async fn commit_statuses(
    State(st): State<crate::api::PrApiState>,
    Path((owner, key, sha)): Path<(String, String, String)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let repo = svc(&st)
        .commit_statuses(&req.0, &full_name(&owner, &key), &sha)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": {
        "combined": repo.combined,
        "checks": repo.checks.iter().map(|c| serde_json::json!({
            "context": c.context,
            "state": c.state.as_str(),
            "description": c.description,
            "target_url": c.target_url,
        })).collect::<Vec<_>>(),
    }}))
    .into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct ProtectionRequest {
    /// 分支模式(精确或 `*` 结尾前缀通配)。
    pub branch_pattern: String,
    /// 合并最少通过人数(0 = 关闭评审门禁)。
    #[serde(default)]
    pub min_approvals: i32,
    /// 必须成功的状态检查上下文。
    #[serde(default)]
    pub required_contexts: Vec<String>,
    /// 禁止 force push(默认 true)。
    #[serde(default = "default_true")]
    pub block_force_push: bool,
    /// 禁止直接推送。
    #[serde(default)]
    pub block_direct_push: bool,
    /// 变更文件所有者必须至少一人通过(T-206)。
    #[serde(default)]
    pub require_code_owner: bool,
}

fn default_true() -> bool {
    true
}

async fn set_protection(
    State(st): State<crate::api::PrApiState>,
    Path((owner, key)): Path<(String, String)>,
    req: axum::Extension<CurrentUser>,
    Json(body): Json<ProtectionRequest>,
) -> Result<Response, AppError> {
    let rule = svc(&st)
        .set_protection(
            &req.0,
            &full_name(&owner, &key),
            &body.branch_pattern,
            body.min_approvals,
            body.required_contexts,
            body.block_force_push,
            body.block_direct_push,
            body.require_code_owner,
        )
        .await?;
    Ok(Json(
        serde_json::json!({"code": "OK", "message": "已保存", "data": {
            "branch_pattern": rule.branch_pattern,
            "min_approvals": rule.min_approvals,
            "required_contexts": rule.required_contexts,
            "block_force_push": rule.block_force_push,
            "block_direct_push": rule.block_direct_push,
            "require_code_owner": rule.require_code_owner,
        }}),
    )
    .into_response())
}

async fn list_protections(
    State(st): State<crate::api::PrApiState>,
    Path((owner, key)): Path<(String, String)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let rules = svc(&st)
        .list_protections(&req.0, &full_name(&owner, &key))
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": rules
        .iter()
        .map(|r| serde_json::json!({
            "branch_pattern": r.branch_pattern,
            "min_approvals": r.min_approvals,
            "required_contexts": r.required_contexts,
            "block_force_push": r.block_force_push,
            "block_direct_push": r.block_direct_push,
            "require_code_owner": r.require_code_owner,
        }))
        .collect::<Vec<_>>()}))
    .into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct DeleteProtectionQuery {
    /// 分支模式。
    pub pattern: String,
}

async fn delete_protection(
    State(st): State<crate::api::PrApiState>,
    Path((owner, key)): Path<(String, String)>,
    req: axum::Extension<CurrentUser>,
    axum::extract::Query(q): axum::extract::Query<DeleteProtectionQuery>,
) -> Result<Response, AppError> {
    svc(&st)
        .delete_protection(&req.0, &full_name(&owner, &key), &q.pattern)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "message": "已删除"})).into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct CodeownersQuery {
    /// 引用(默认仓库默认分支)。
    pub r#ref: Option<String>,
}

async fn codeowners_info(
    State(st): State<crate::api::PrApiState>,
    Path((owner, key)): Path<(String, String)>,
    req: axum::Extension<CurrentUser>,
    axum::extract::Query(q): axum::extract::Query<CodeownersQuery>,
) -> Result<Response, AppError> {
    let (source, raw, rules) = svc(&st)
        .codeowners_info(&req.0, &full_name(&owner, &key), q.r#ref.as_deref())
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": {
        "source": source,
        "raw": raw,
        "rules": rules.iter().map(|r| serde_json::json!({
            "pattern": r.pattern,
            "owners": r.owners,
        })).collect::<Vec<_>>(),
    }}))
    .into_response())
}

async fn list_reviewers(
    State(st): State<crate::api::PrApiState>,
    Path((owner, key, number)): Path<(String, String, i64)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let reviewers = svc(&st)
        .list_reviewers(&req.0, &full_name(&owner, &key), number)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": reviewers
        .iter()
        .map(|(r, v)| serde_json::json!({
            "reviewer": r.reviewer.as_uuid(),
            "added_by": r.added_by.map(|u| u.as_uuid()),
            "verdict": v.map(|x| x.as_str()),
            "created_at": r.created_at.to_rfc3339(),
        }))
        .collect::<Vec<_>>()}))
    .into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct AddReviewerRequest {
    /// 被指派用户登录名。
    pub username: String,
}

async fn add_reviewer(
    State(st): State<crate::api::PrApiState>,
    Path((owner, key, number)): Path<(String, String, i64)>,
    req: axum::Extension<CurrentUser>,
    Json(body): Json<AddReviewerRequest>,
) -> Result<Response, AppError> {
    let (reviewer, user) = svc(&st)
        .add_reviewer(&req.0, &full_name(&owner, &key), number, &body.username)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": {
        "reviewer": reviewer.reviewer.as_uuid(),
        "username": body.username,
        "display_name": user.display_name,
    }}))
    .into_response())
}

async fn remove_reviewer(
    State(st): State<crate::api::PrApiState>,
    Path((owner, key, number, username)): Path<(String, String, i64, String)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    svc(&st)
        .remove_reviewer(&req.0, &full_name(&owner, &key), number, &username)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK"})).into_response())
}
