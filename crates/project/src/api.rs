//! HTTP 层:项目/工作项路由。认证经 kernel::TokenAuthenticator 中间件注入
//! `CurrentUser`(域不依赖 identity);`AppError` 的 HTTP 适配来自 common(feature axum)。

use axum::extract::{Path, Query, State};
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
use utoipa::{IntoParams, ToSchema};

use crate::DOMAIN_CODE;
use crate::domain::{ItemType, NewWorkItem, Priority};
use crate::service::ProjectService;

/// api 层状态。
#[derive(Clone)]
pub struct ProjectState {
    /// 项目域服务。
    pub svc: Arc<ProjectService>,
    /// 令牌认证器(实现由 server 注入 identity)。
    pub authn: Arc<dyn TokenAuthenticator>,
}

/// 路由(自带认证中间件;server 侧 `.with_state` 后聚合)。
pub fn routes(state: ProjectState) -> Router<ProjectState> {
    Router::new()
        .route("/api/projects", post(create_project).get(list_projects))
        .route(
            "/api/projects/{key}/items",
            post(create_item).get(list_items),
        )
        .route("/api/projects/{key}/items/{number}", get(get_item))
        .route("/api/workitems/{id}/transition", post(transition))
        .route("/api/workitems/{id}", axum::routing::patch(update_item))
        .layer(middleware::from_fn_with_state(state, auth_mw))
}

/// 认证中间件:Bearer → `CurrentUser` 注入请求扩展。
async fn auth_mw(
    State(st): State<ProjectState>,
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

/// 项目响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct ProjectResponse {
    /// 项目 ID。
    pub id: String,
    /// 项目键。
    pub key: String,
    /// 名称。
    pub name: String,
    /// 描述。
    pub description: String,
    /// 创建时间(RFC 3339)。
    pub created_at: String,
}

/// 创建项目请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateProjectRequest {
    /// 项目键(大写字母/数字/连字符,2~16 位,工作项编号前缀)。
    pub key: String,
    /// 名称。
    pub name: String,
    /// 描述。
    #[serde(default)]
    pub description: String,
}

/// 工作项响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct ItemResponse {
    /// 工作项 ID。
    pub id: String,
    /// 项目内编号(展示编号 = `{项目键}-{编号}`)。
    pub number: i64,
    /// 类型:epic/story/task/bug。
    pub item_type: String,
    /// 标题。
    pub title: String,
    /// 状态。
    pub status: String,
    /// 优先级。
    pub priority: String,
    /// 经办人。
    pub assignee: Option<String>,
    /// 自定义字段。
    pub custom_fields: serde_json::Value,
}

/// 创建工作项请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateItemRequest {
    /// 类型:epic/story/task/bug。
    pub item_type: String,
    /// 标题。
    pub title: String,
    /// 描述(Markdown)。
    #[serde(default)]
    pub description: String,
    /// 优先级(highest/high/medium/low/lowest)。
    pub priority: Option<String>,
}

/// 工作项列表查询参数。
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListItemsQuery {
    /// 按类型过滤。
    pub item_type: Option<String>,
    /// 按状态过滤。
    pub status: Option<String>,
}

/// 状态流转请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct TransitionRequest {
    /// 目标状态。
    pub to: String,
}

/// 更新工作项请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateItemRequest {
    /// 新标题。
    pub title: Option<String>,
    /// 新描述。
    pub description: Option<String>,
    /// 新优先级。
    pub priority: Option<String>,
}

fn item_response(item: &crate::domain::WorkItem) -> ItemResponse {
    ItemResponse {
        id: item.id.as_uuid().to_string(),
        number: item.item_number,
        item_type: item.item_type.as_str().to_owned(),
        title: item.title.clone(),
        status: item.status.clone(),
        priority: item.priority.as_str().to_owned(),
        assignee: item.assignee.map(|u| u.as_uuid().to_string()),
        custom_fields: item.custom_fields.clone(),
    }
}

/// 创建项目(发布 `project.created`,创建者自动成为 admin)。
#[utoipa::path(
    post,
    path = "/api/projects",
    request_body = CreateProjectRequest,
    responses((status = 200, body = ProjectResponse), (status = 409))
)]
async fn create_project(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<Json<ApiResponse<ProjectResponse>>, AppError> {
    let project = st
        .svc
        .create_project(&user, &req.key, &req.name, &req.description)
        .await?;
    Ok(Json(ApiResponse::ok(ProjectResponse {
        id: project.id.as_uuid().to_string(),
        key: project.key,
        name: project.name,
        description: project.description,
        created_at: chrono::DateTime::to_rfc3339(&project.created_at),
    })))
}

/// 项目列表。
#[utoipa::path(get, path = "/api/projects", responses((status = 200, body = ProjectResponse)))]
async fn list_projects(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
) -> Result<Json<ApiResponse<Vec<ProjectResponse>>>, AppError> {
    let _ = user;
    let projects = st.svc.list_projects().await?;
    Ok(Json(ApiResponse::ok(
        projects
            .into_iter()
            .map(|p| ProjectResponse {
                id: p.id.as_uuid().to_string(),
                key: p.key,
                name: p.name,
                description: p.description,
                created_at: chrono::DateTime::to_rfc3339(&p.created_at),
            })
            .collect(),
    )))
}

/// 创建工作项。
#[utoipa::path(
    post,
    path = "/api/projects/{key}/items",
    request_body = CreateItemRequest,
    params(("key" = String, Path, description = "项目键")),
    responses((status = 200, body = ItemResponse), (status = 403))
)]
async fn create_item(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(key): Path<String>,
    Json(req): Json<CreateItemRequest>,
) -> Result<Json<ApiResponse<ItemResponse>>, AppError> {
    let item_type = ItemType::from_str_opt(&req.item_type)
        .ok_or_else(|| AppError::invalid(DOMAIN_CODE, format!("非法类型:{}", req.item_type)))?;
    let priority = req.priority.as_deref().and_then(Priority::from_str_opt);
    let item = st
        .svc
        .create_item(
            &user,
            &key,
            &NewWorkItem {
                item_type,
                title: req.title,
                description: req.description,
                priority,
                assignee: None,
                reporter: Some(user.id),
                custom_fields: serde_json::json!({}),
            },
        )
        .await?;
    Ok(Json(ApiResponse::ok(item_response(&item))))
}

/// 工作项列表(类型/状态过滤,编号倒序)。
#[utoipa::path(
    get,
    path = "/api/projects/{key}/items",
    params(ListItemsQuery, ("key" = String, Path)),
    responses((status = 200, body = ItemResponse))
)]
async fn list_items(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(key): Path<String>,
    Query(q): Query<ListItemsQuery>,
) -> Result<Json<ApiResponse<Vec<ItemResponse>>>, AppError> {
    let item_type = q.item_type.as_deref().and_then(ItemType::from_str_opt);
    let items = st
        .svc
        .list_items(&user, &key, item_type, q.status.as_deref())
        .await?;
    Ok(Json(ApiResponse::ok(
        items.iter().map(item_response).collect(),
    )))
}

/// 工作项路径参数(键 + 编号)。
#[derive(Debug, Deserialize)]
pub struct KeyNumberPath {
    /// 项目键。
    pub key: String,
    /// 项目内编号。
    pub number: i64,
}

/// 按展示编号取单个工作项。
#[utoipa::path(
    get,
    path = "/api/projects/{key}/items/{number}",
    params(("key" = String, Path), ("number" = i64, Path)),
    responses((status = 200, body = ItemResponse), (status = 404))
)]
async fn get_item(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(p): Path<KeyNumberPath>,
) -> Result<Json<ApiResponse<ItemResponse>>, AppError> {
    let _ = user;
    let items = st.svc.list_items_public(&p.key).await?;
    let item = items
        .into_iter()
        .find(|i| i.item_number == p.number)
        .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "工作项不存在"))?;
    Ok(Json(ApiResponse::ok(item_response(&item))))
}

/// 工作项状态流转。
#[utoipa::path(
    post,
    path = "/api/workitems/{id}/transition",
    request_body = TransitionRequest,
    params(("id" = String, Path)),
    responses((status = 200, body = ItemResponse), (status = 409), (status = 403))
)]
async fn transition(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(id): Path<String>,
    Json(req): Json<TransitionRequest>,
) -> Result<Json<ApiResponse<ItemResponse>>, AppError> {
    let item_id = crate::domain::WorkItemId::try_from(id.as_str())
        .map_err(|_| AppError::invalid(DOMAIN_CODE, "非法工作项 ID"))?;
    let updated = st.svc.transition(&user, item_id, &req.to).await?;
    Ok(Json(ApiResponse::ok(item_response(&updated))))
}

/// 更新工作项字段。
#[utoipa::path(
    patch,
    path = "/api/workitems/{id}",
    request_body = UpdateItemRequest,
    params(("id" = String, Path)),
    responses((status = 200, body = ItemResponse))
)]
async fn update_item(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateItemRequest>,
) -> Result<Json<ApiResponse<ItemResponse>>, AppError> {
    let item_id = crate::domain::WorkItemId::try_from(id.as_str())
        .map_err(|_| AppError::invalid(DOMAIN_CODE, "非法工作项 ID"))?;
    let priority = req.priority.as_deref().and_then(Priority::from_str_opt);
    let updated = st
        .svc
        .update_item(
            &user,
            item_id,
            req.title.as_deref(),
            req.description.as_deref(),
            priority,
            None,
        )
        .await?;
    Ok(Json(ApiResponse::ok(item_response(&updated))))
}
