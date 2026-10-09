//! HTTP 层:Wiki 路由。认证同项目域(kernel::TokenAuthenticator 中间件)。
//! 路径使用项目 ID(前端由 /api/projects 持有),避免域间按 key 解析。

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::header::AUTHORIZATION;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router, middleware};
use codeplan_common::error::AppError;
use codeplan_common::response::ApiResponse;
use codeplan_kernel::authn::TokenAuthenticator;
use codeplan_kernel::identity::CurrentUser;
use codeplan_kernel::ids::ProjectId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

use crate::DOMAIN_CODE;
use crate::domain::{PageId, PageUpsert};
use crate::service::WikiService;

/// api 层状态。
#[derive(Clone)]
pub struct WikiState {
    /// Wiki 域服务。
    pub svc: Arc<WikiService>,
    /// 令牌认证器。
    pub authn: Arc<dyn TokenAuthenticator>,
}

/// 路由(自带认证中间件)。
pub fn routes(state: WikiState) -> Router<WikiState> {
    Router::new()
        .route(
            "/api/projects/{project_id}/wiki",
            post(create_page).get(page_tree),
        )
        .route(
            "/api/projects/{project_id}/wiki/{slug}",
            get(get_page).put(update_page).delete(delete_page),
        )
        .route(
            "/api/projects/{project_id}/wiki/{slug}/attachments",
            post(upload_attachment),
        )
        .route("/api/attachments", get(list_attachments))
        .route("/api/attachments/{id}", get(download_attachment))
        .layer(middleware::from_fn_with_state(state, auth_mw))
}

/// 认证中间件。
async fn auth_mw(
    State(st): State<WikiState>,
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

/// 页面响应(源 + 渲染 HTML)。
#[derive(Debug, Serialize, ToSchema)]
pub struct PageResponse {
    /// 页面 ID。
    pub id: String,
    /// slug。
    pub slug: String,
    /// 标题。
    pub title: String,
    /// Markdown 源。
    pub content: String,
    /// 渲染 HTML(comrak GFM)。
    pub html: String,
    /// 父页面。
    pub parent_id: Option<String>,
    /// 更新时间(RFC 3339)。
    pub updated_at: String,
}

/// 创建/更新页面请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct PageUpsertRequest {
    /// slug(仅创建时需要;项目内唯一,小写字母/数字/连字符)。
    pub slug: String,
    /// 标题。
    pub title: String,
    /// Markdown 内容。
    #[serde(default)]
    pub content: String,
    /// 父页面 ID。
    pub parent_id: Option<String>,
}

/// 附件元数据响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct AttachmentResponse {
    /// 附件 ID。
    pub id: String,
    /// 文件名。
    pub filename: String,
    /// 字节数。
    pub size_bytes: i64,
    /// 内容类型。
    pub content_type: Option<String>,
}

fn page_response(page: &crate::domain::WikiPage, html: &str) -> PageResponse {
    PageResponse {
        id: page.id.as_uuid().to_string(),
        slug: page.slug.clone(),
        title: page.title.clone(),
        content: page.content.clone(),
        html: html.to_owned(),
        parent_id: page.parent_id.map(|p| p.as_uuid().to_string()),
        updated_at: chrono::DateTime::to_rfc3339(&page.updated_at),
    }
}

fn upsert_from(req: PageUpsertRequest) -> Result<PageUpsert, AppError> {
    Ok(PageUpsert {
        title: req.title,
        content: req.content,
        parent_id: req
            .parent_id
            .as_deref()
            .map(PageId::try_from)
            .transpose()
            .map_err(|_| AppError::invalid(DOMAIN_CODE, "非法父页面 ID"))?,
    })
}

/// 创建页面。
#[utoipa::path(
    post,
    path = "/api/projects/{project_id}/wiki",
    request_body = PageUpsertRequest,
    params(("project_id" = String, Path)),
    responses((status = 200, body = PageResponse), (status = 409))
)]
async fn create_page(
    Extension(user): Extension<CurrentUser>,
    State(st): State<WikiState>,
    Path(project_id): Path<ProjectId>,
    Json(req): Json<PageUpsertRequest>,
) -> Result<Json<ApiResponse<PageResponse>>, AppError> {
    let slug = req.slug.clone();
    let upsert = upsert_from(req)?;
    let page = st
        .svc
        .create_page(&user, &project_id, &slug, &upsert)
        .await?;
    let html = crate::service::render_markdown(&page.content);
    Ok(Json(ApiResponse::ok(page_response(&page, &html))))
}

/// 文档树。
#[utoipa::path(
    get,
    path = "/api/projects/{project_id}/wiki",
    params(("project_id" = String, Path)),
    responses((status = 200))
)]
async fn page_tree(
    Extension(user): Extension<CurrentUser>,
    State(st): State<WikiState>,
    Path(project_id): Path<ProjectId>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let tree = st.svc.page_tree(&user, &project_id).await?;
    Ok(Json(ApiResponse::ok(serde_json::Value::Array(tree))))
}

/// 取页面(源 + HTML)。
#[utoipa::path(
    get,
    path = "/api/projects/{project_id}/wiki/{slug}",
    params(("project_id" = String, Path), ("slug" = String, Path)),
    responses((status = 200, body = PageResponse), (status = 404))
)]
async fn get_page(
    Extension(user): Extension<CurrentUser>,
    State(st): State<WikiState>,
    Path((project_id, slug)): Path<(ProjectId, String)>,
) -> Result<Json<ApiResponse<PageResponse>>, AppError> {
    let (page, html) = st.svc.page_by_slug(&user, &project_id, &slug).await?;
    Ok(Json(ApiResponse::ok(page_response(&page, &html))))
}

/// 更新页面。
#[utoipa::path(
    put,
    path = "/api/projects/{project_id}/wiki/{slug}",
    request_body = PageUpsertRequest,
    responses((status = 200, body = PageResponse))
)]
async fn update_page(
    Extension(user): Extension<CurrentUser>,
    State(st): State<WikiState>,
    Path((project_id, slug)): Path<(ProjectId, String)>,
    Json(req): Json<PageUpsertRequest>,
) -> Result<Json<ApiResponse<PageResponse>>, AppError> {
    let (existing, _) = st.svc.page_by_slug(&user, &project_id, &slug).await?;
    let upsert = upsert_from(req)?;
    let page = st.svc.update_page(&user, existing.id, &upsert).await?;
    let html = crate::service::render_markdown(&page.content);
    Ok(Json(ApiResponse::ok(page_response(&page, &html))))
}

/// 删除页面(级联附件)。
#[utoipa::path(
    delete,
    path = "/api/projects/{project_id}/wiki/{slug}",
    params(("project_id" = String, Path), ("slug" = String, Path)),
    responses((status = 200))
)]
async fn delete_page(
    Extension(user): Extension<CurrentUser>,
    State(st): State<WikiState>,
    Path((project_id, slug)): Path<(ProjectId, String)>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let (page, _) = st.svc.page_by_slug(&user, &project_id, &slug).await?;
    st.svc.delete_page(&user, page.id).await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "已删除".to_owned(),
        data: None,
    }))
}

/// 上传附件查询参数。
#[derive(Debug, Deserialize, IntoParams)]
pub struct UploadQuery {
    /// 文件名。
    pub filename: String,
    /// 内容类型(可选)。
    pub content_type: Option<String>,
}

/// 上传附件(请求体为原始字节)。
#[utoipa::path(
    post,
    path = "/api/projects/{project_id}/wiki/{slug}/attachments",
    params(UploadQuery, ("project_id" = String, Path), ("slug" = String, Path)),
    request_body(content = String, description = "原始文件字节"),
    responses((status = 200, body = AttachmentResponse))
)]
async fn upload_attachment(
    Extension(user): Extension<CurrentUser>,
    State(st): State<WikiState>,
    Path((project_id, slug)): Path<(ProjectId, String)>,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> Result<Json<ApiResponse<AttachmentResponse>>, AppError> {
    let (page, _) = st.svc.page_by_slug(&user, &project_id, &slug).await?;
    let att = st
        .svc
        .save_attachment(
            &user,
            page.id,
            &q.filename,
            q.content_type.as_deref(),
            &body,
        )
        .await?;
    Ok(Json(ApiResponse::ok(AttachmentResponse {
        id: att.id.as_uuid().to_string(),
        filename: att.filename,
        size_bytes: att.size_bytes,
        content_type: att.content_type,
    })))
}

/// 附件列表查询参数。
#[derive(Debug, Deserialize, IntoParams)]
pub struct AttachmentListQuery {
    /// 页面 ID。
    pub page_id: String,
}

/// 页面附件列表。
#[utoipa::path(
    get,
    path = "/api/attachments",
    params(AttachmentListQuery),
    responses((status = 200, body = AttachmentResponse))
)]
async fn list_attachments(
    Extension(user): Extension<CurrentUser>,
    State(st): State<WikiState>,
    Query(q): Query<AttachmentListQuery>,
) -> Result<Json<ApiResponse<Vec<AttachmentResponse>>>, AppError> {
    let page_id = PageId::try_from(q.page_id.as_str())
        .map_err(|_| AppError::invalid(DOMAIN_CODE, "非法页面 ID"))?;
    let list = st.svc.list_attachments(&user, page_id).await?;
    Ok(Json(ApiResponse::ok(
        list.into_iter()
            .map(|a| AttachmentResponse {
                id: a.id.as_uuid().to_string(),
                filename: a.filename,
                size_bytes: a.size_bytes,
                content_type: a.content_type,
            })
            .collect(),
    )))
}

/// 下载附件。
#[utoipa::path(
    get,
    path = "/api/attachments/{id}",
    params(("id" = String, Path)),
    responses((status = 200), (status = 404))
)]
async fn download_attachment(
    Extension(user): Extension<CurrentUser>,
    State(st): State<WikiState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let att_id = crate::domain::AttachmentId::try_from(id.as_str())
        .map_err(|_| AppError::invalid(DOMAIN_CODE, "非法附件 ID"))?;
    let (att, bytes) = st.svc.read_attachment(&user, att_id).await?;
    let mut resp = (axum::http::StatusCode::OK, bytes).into_response();
    if let Ok(v) = axum::http::HeaderValue::from_str(&att.filename.replace(['"', '\n', '\r'], ""))
        && let Ok(cd) = axum::http::HeaderValue::from_str(&format!(
            "attachment; filename=\"{}\"",
            v.to_str().unwrap_or_default()
        ))
    {
        resp.headers_mut()
            .insert(axum::http::header::CONTENT_DISPOSITION, cd);
    }
    if let Some(ct) = &att.content_type
        && let Ok(v) = axum::http::HeaderValue::from_str(ct)
    {
        resp.headers_mut()
            .insert(axum::http::header::CONTENT_TYPE, v);
    }
    Ok(resp)
}
