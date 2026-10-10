//! 代码浏览路由(T-122):文件树/提交历史/提交详情/blob/diff/blame 只读 API。

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json};
use codeplan_common::error::AppError;
use codeplan_common::response::ApiResponse;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use codeplan_kernel::identity::CurrentUser;

use crate::api::GitState;
use crate::browse::GitBrowse;

async fn browse_of(
    st: &GitState,
    repo: &crate::domain::Repository,
) -> Result<(GitBrowse, String), AppError> {
    let path = format!(
        "{}/{full_name}.git",
        st.repo_root,
        full_name = repo.full_name
    );
    Ok((
        GitBrowse::open(std::path::Path::new(&path))?,
        repo.default_branch.clone(),
    ))
}

/// 从查询或默认引用解析。
fn ref_or<'a>(q: &'a RefQuery, default_ref: &'a str) -> &'a str {
    q.r#ref.as_deref().unwrap_or(default_ref)
}

/// 树条目响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct TreeEntryResponse {
    /// 名称。
    pub name: String,
    /// 类型:tree/blob。
    pub kind: String,
    /// OID。
    pub oid: String,
    /// 大小(blob)。
    pub size: Option<i64>,
}

/// 提交响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct CommitResponse {
    /// OID。
    pub id: String,
    /// 摘要。
    pub summary: String,
    /// 作者名。
    pub author_name: String,
    /// 作者邮箱。
    pub author_email: String,
    /// 时间(Unix 秒)。
    pub time: i64,
}

/// 变更文件响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct ChangeResponse {
    /// 文件路径。
    pub path: String,
    /// 状态:A/M/D。
    pub status: String,
}

/// blame 行块响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct BlameResponse {
    /// 提交 OID。
    pub commit: String,
    /// 作者。
    pub author: String,
    /// 起始行。
    pub start_line: usize,
    /// 行数。
    pub line_count: usize,
}

/// 引用查询参数。
#[derive(Debug, Deserialize, IntoParams)]
pub struct RefQuery {
    /// 引用(分支/标签/HEAD,默认 default_branch)。
    pub r#ref: Option<String>,
    /// 子路径。
    pub path: Option<String>,
    /// 历史条数上限。
    pub limit: Option<usize>,
}

async fn tree_entries(
    Extension(user): Extension<CurrentUser>,
    State(st): State<GitState>,
    Path((owner, key)): Path<(String, String)>,
    Query(q): Query<RefQuery>,
) -> Result<Json<ApiResponse<Vec<TreeEntryResponse>>>, AppError> {
    let _ = user;
    let repo = st
        .svc
        .repository_by_full_name(&format!("{owner}/{key}"))
        .await?;
    let (browse, default_ref) = browse_of(&st, &repo).await?;
    let reference = ref_or(&q, &default_ref);
    let entries = browse
        .tree_entries(reference, q.path.as_deref().unwrap_or(""))
        .await?;
    Ok(Json(ApiResponse::ok(
        entries
            .into_iter()
            .map(|e| TreeEntryResponse {
                name: e.name,
                kind: e.kind,
                oid: e.oid,
                size: e.size,
            })
            .collect(),
    )))
}

async fn commit_log(
    Extension(user): Extension<CurrentUser>,
    State(st): State<GitState>,
    Path((owner, key)): Path<(String, String)>,
    Query(q): Query<RefQuery>,
) -> Result<Json<ApiResponse<Vec<CommitResponse>>>, AppError> {
    let _ = user;
    let repo = st
        .svc
        .repository_by_full_name(&format!("{owner}/{key}"))
        .await?;
    let (browse, default_ref) = browse_of(&st, &repo).await?;
    let reference = ref_or(&q, &default_ref);
    let log = browse
        .commit_log(reference, q.limit.unwrap_or(50).clamp(1, 200))
        .await?;
    Ok(Json(ApiResponse::ok(
        log.into_iter()
            .map(|c| CommitResponse {
                id: c.id,
                summary: c.summary,
                author_name: c.author_name,
                author_email: c.author_email,
                time: c.time,
            })
            .collect(),
    )))
}

async fn commit_detail(
    Extension(user): Extension<CurrentUser>,
    State(st): State<GitState>,
    Path((owner, key, oid)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _ = user;
    let repo = st
        .svc
        .repository_by_full_name(&format!("{owner}/{key}"))
        .await?;
    let (browse, _) = browse_of(&st, &repo).await?;
    let (dto, changes) = browse.commit_detail(&oid).await?;
    Ok(Json(serde_json::json!({
        "code": "OK",
        "data": {
            "id": dto.id,
            "summary": dto.summary,
            "author_name": dto.author_name,
            "author_email": dto.author_email,
            "time": dto.time,
            "changes": changes.into_iter().map(|(path, status)| {
                serde_json::json!({"path": path, "status": status})
            }).collect::<Vec<_>>(),
        }
    })))
}

async fn blob_content(
    Extension(user): Extension<CurrentUser>,
    State(st): State<GitState>,
    Path((owner, key)): Path<(String, String)>,
    Query(q): Query<RefQuery>,
) -> Result<Response, AppError> {
    let _ = user;
    let repo = st
        .svc
        .repository_by_full_name(&format!("{owner}/{key}"))
        .await?;
    let (browse, default_ref) = browse_of(&st, &repo).await?;
    let reference = ref_or(&q, &default_ref);
    let path = q.path.clone().unwrap_or_default();
    let bytes = browse.blob_content(reference, &path).await?;
    let mut resp = (axum::http::StatusCode::OK, bytes).into_response();
    resp.headers_mut().insert(
        CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    Ok(resp)
}

async fn diff_files(
    Extension(user): Extension<CurrentUser>,
    State(st): State<GitState>,
    Path((owner, key)): Path<(String, String)>,
    Query(q): Query<RefQuery>,
) -> Result<Json<ApiResponse<Vec<ChangeResponse>>>, AppError> {
    let _ = user;
    let repo = st
        .svc
        .repository_by_full_name(&format!("{owner}/{key}"))
        .await?;
    let (browse, default_ref) = browse_of(&st, &repo).await?;
    let to_ref = q.r#ref.unwrap_or(default_ref);
    let changes = browse.diff_names(&to_ref, "HEAD").await?;
    Ok(Json(ApiResponse::ok(
        changes
            .into_iter()
            .map(|(path, status)| ChangeResponse {
                path,
                status: status.to_string(),
            })
            .collect(),
    )))
}

async fn blame_view(
    Extension(user): Extension<CurrentUser>,
    State(st): State<GitState>,
    Path((owner, key)): Path<(String, String)>,
    Query(q): Query<RefQuery>,
) -> Result<Json<ApiResponse<Vec<BlameResponse>>>, AppError> {
    let _ = user;
    let repo = st
        .svc
        .repository_by_full_name(&format!("{owner}/{key}"))
        .await?;
    let (browse, default_ref) = browse_of(&st, &repo).await?;
    let reference = ref_or(&q, &default_ref);
    let path = q.path.clone().unwrap_or_default();
    let entries = browse.blame(reference, &path).await?;
    Ok(Json(ApiResponse::ok(
        entries
            .into_iter()
            .map(|e| BlameResponse {
                commit: e.commit,
                author: e.author,
                start_line: e.start_line,
                line_count: e.line_count,
            })
            .collect(),
    )))
}

/// 浏览路由挂载(并入 git api 路由)。
pub fn browse_routes() -> axum::Router<GitState> {
    Router::new()
        .route("/api/repos/{owner}/{key}/tree", get(tree_entries))
        .route("/api/repos/{owner}/{key}/commits", get(commit_log))
        .route("/api/repos/{owner}/{key}/commits/{oid}", get(commit_detail))
        .route("/api/repos/{owner}/{key}/blob", get(blob_content))
        .route("/api/repos/{owner}/{key}/diff", get(diff_files))
        .route("/api/repos/{owner}/{key}/blame", get(blame_view))
}
