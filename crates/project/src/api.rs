//! HTTP 层:项目/工作项路由。认证经 kernel::TokenAuthenticator 中间件注入
//! `CurrentUser`(域不依赖 identity);`AppError` 的 HTTP 适配来自 common(feature axum)。

use axum::extract::{Path, Query, State};
use axum::http::header::AUTHORIZATION;
use axum::response::Response;
use axum::routing::{get, post, put};
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
        .route(
            "/api/projects/{key}/sprints",
            post(create_sprint).get(list_sprints),
        )
        .route("/api/sprints/{id}/start", post(start_sprint))
        .route("/api/sprints/{id}/complete", post(complete_sprint))
        .route("/api/workitems/{id}/sprint", put(set_item_sprint))
        .route(
            "/api/projects/{key}/milestones",
            post(create_milestone).get(list_milestones),
        )
        .route("/api/milestones/{id}/close", post(close_milestone))
        .route("/api/workitems/{id}/milestone", put(set_item_milestone))
        .route("/api/projects/{key}/board", get(board_view))
        .route("/api/projects/{key}/board/reorder", put(board_reorder))
        .route("/api/projects/{key}/items/tree", get(items_tree))
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
    /// 父工作项 ID(需求树:epic → story → task)。
    pub parent_id: Option<String>,
    /// 自定义字段(缺陷:severity/repro_steps,服务端类型化校验)。
    pub custom_fields: Option<serde_json::Value>,
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
    /// 自定义字段(增量合并语义由服务端决定;缺陷类型会做规范化校验)。
    pub custom_fields: Option<serde_json::Value>,
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

/// 取缺陷严重程度(custom_fields.severity)。
#[allow(dead_code)]
fn severity_of(item: &crate::domain::WorkItem) -> Option<&str> {
    if item.item_type != ItemType::Bug {
        return None;
    }
    item.custom_fields.get("severity").and_then(|v| v.as_str())
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
                custom_fields: req.custom_fields.unwrap_or_else(|| serde_json::json!({})),
                parent_id: req
                    .parent_id
                    .as_deref()
                    .map(crate::domain::WorkItemId::try_from)
                    .transpose()
                    .map_err(|_| AppError::invalid(DOMAIN_CODE, "非法父工作项 ID"))?,
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
            req.custom_fields.as_ref(),
        )
        .await?;
    Ok(Json(ApiResponse::ok(item_response(&updated))))
}

// ---------- 迭代与里程碑(T-112) ----------

/// 创建迭代请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSprintRequest {
    /// 名称。
    pub name: String,
    /// 目标。
    #[serde(default)]
    pub goal: String,
    /// 开始时间(RFC 3339)。
    pub starts_at: Option<String>,
    /// 结束时间(RFC 3339)。
    pub ends_at: Option<String>,
    /// 容量点数。
    #[serde(default)]
    pub capacity_points: i32,
}

/// 迭代响应(含进度)。
#[derive(Debug, Serialize, ToSchema)]
pub struct SprintResponse {
    /// 迭代 ID。
    pub id: String,
    /// 名称。
    pub name: String,
    /// 目标。
    pub goal: String,
    /// 状态:planned/active/completed。
    pub status: String,
    /// 容量点数。
    pub capacity_points: i32,
    /// 工作项总数。
    pub total: i64,
    /// 已完成数。
    pub done: i64,
    /// 提交点数。
    pub committed_points: i64,
}

fn parse_time(s: &str) -> Result<chrono::DateTime<chrono::Utc>, AppError> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&chrono::Utc))
        .map_err(|_| AppError::invalid(DOMAIN_CODE, format!("非法时间:{s}")))
}

async fn create_sprint(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(key): Path<String>,
    Json(req): Json<CreateSprintRequest>,
) -> Result<Json<ApiResponse<SprintResponse>>, AppError> {
    let starts_at = req.starts_at.as_deref().map(parse_time).transpose()?;
    let ends_at = req.ends_at.as_deref().map(parse_time).transpose()?;
    let sprint = st
        .svc
        .create_sprint(
            &user,
            &key,
            &crate::domain::NewSprint {
                name: req.name,
                goal: req.goal,
                starts_at,
                ends_at,
                capacity_points: req.capacity_points,
            },
        )
        .await?;
    Ok(Json(ApiResponse::ok(SprintResponse {
        id: sprint.id.as_uuid().to_string(),
        name: sprint.name,
        goal: sprint.goal,
        status: sprint.status.as_str().to_owned(),
        capacity_points: sprint.capacity_points,
        total: 0,
        done: 0,
        committed_points: 0,
    })))
}

async fn list_sprints(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(key): Path<String>,
) -> Result<Json<ApiResponse<Vec<SprintResponse>>>, AppError> {
    let pairs = st.svc.list_sprints(&user, &key).await?;
    Ok(Json(ApiResponse::ok(
        pairs
            .into_iter()
            .map(|(s, p)| SprintResponse {
                id: s.id.as_uuid().to_string(),
                name: s.name,
                goal: s.goal,
                status: s.status.as_str().to_owned(),
                capacity_points: s.capacity_points,
                total: p.total,
                done: p.done,
                committed_points: p.committed_points,
            })
            .collect(),
    )))
}

async fn start_sprint(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let sprint_id = crate::domain::SprintId::try_from(id.as_str())
        .map_err(|_| AppError::invalid(DOMAIN_CODE, "非法迭代 ID"))?;
    st.svc.start_sprint(&user, sprint_id).await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "迭代已启动".to_owned(),
        data: None,
    }))
}

async fn complete_sprint(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let sprint_id = crate::domain::SprintId::try_from(id.as_str())
        .map_err(|_| AppError::invalid(DOMAIN_CODE, "非法迭代 ID"))?;
    st.svc.complete_sprint(&user, sprint_id).await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "迭代已完成".to_owned(),
        data: None,
    }))
}

/// 工作项关联迭代请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct SetSprintRequest {
    /// 迭代 ID(null 移除)。
    pub sprint_id: Option<String>,
}

async fn set_item_sprint(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(id): Path<String>,
    Json(req): Json<SetSprintRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let item_id = crate::domain::WorkItemId::try_from(id.as_str())
        .map_err(|_| AppError::invalid(DOMAIN_CODE, "非法工作项 ID"))?;
    let sprint_id = match req.sprint_id {
        Some(s) => Some(
            crate::domain::SprintId::try_from(s.as_str())
                .map_err(|_| AppError::invalid(DOMAIN_CODE, "非法迭代 ID"))?,
        ),
        None => None,
    };
    st.svc.set_item_sprint(&user, item_id, sprint_id).await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "已关联".to_owned(),
        data: None,
    }))
}

/// 创建里程碑请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateMilestoneRequest {
    /// 名称。
    pub name: String,
    /// 描述。
    #[serde(default)]
    pub description: String,
    /// 截止日期(RFC 3339)。
    pub due_date: Option<String>,
}

/// 里程碑响应(含进度)。
#[derive(Debug, Serialize, ToSchema)]
pub struct MilestoneResponse {
    /// 里程碑 ID。
    pub id: String,
    /// 名称。
    pub name: String,
    /// 状态:open/closed。
    pub status: String,
    /// 截止日期(RFC 3339)。
    pub due_date: Option<String>,
    /// 工作项总数。
    pub total: i64,
    /// 已完成数。
    pub done: i64,
}

async fn create_milestone(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(key): Path<String>,
    Json(req): Json<CreateMilestoneRequest>,
) -> Result<Json<ApiResponse<MilestoneResponse>>, AppError> {
    let due_date = req.due_date.as_deref().map(parse_time).transpose()?;
    let m = st
        .svc
        .create_milestone(
            &user,
            &key,
            &crate::domain::NewMilestone {
                name: req.name,
                description: req.description,
                due_date,
            },
        )
        .await?;
    Ok(Json(ApiResponse::ok(MilestoneResponse {
        id: m.id.as_uuid().to_string(),
        name: m.name,
        status: m.status,
        due_date: m.due_date.map(|d| d.to_rfc3339()),
        total: 0,
        done: 0,
    })))
}

async fn list_milestones(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(key): Path<String>,
) -> Result<Json<ApiResponse<Vec<MilestoneResponse>>>, AppError> {
    let pairs = st.svc.list_milestones(&user, &key).await?;
    Ok(Json(ApiResponse::ok(
        pairs
            .into_iter()
            .map(|(m, p)| MilestoneResponse {
                id: m.id.as_uuid().to_string(),
                name: m.name,
                status: m.status,
                due_date: m.due_date.map(|d| d.to_rfc3339()),
                total: p.total,
                done: p.done,
            })
            .collect(),
    )))
}

async fn close_milestone(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let milestone_id = crate::domain::MilestoneId::try_from(id.as_str())
        .map_err(|_| AppError::invalid(DOMAIN_CODE, "非法里程碑 ID"))?;
    st.svc.close_milestone(&user, milestone_id).await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "里程碑已关闭".to_owned(),
        data: None,
    }))
}

/// 工作项关联里程碑请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct SetMilestoneRequest {
    /// 里程碑 ID(null 移除)。
    pub milestone_id: Option<String>,
}

async fn set_item_milestone(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(id): Path<String>,
    Json(req): Json<SetMilestoneRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let item_id = crate::domain::WorkItemId::try_from(id.as_str())
        .map_err(|_| AppError::invalid(DOMAIN_CODE, "非法工作项 ID"))?;
    let milestone_id = match req.milestone_id {
        Some(s) => Some(
            crate::domain::MilestoneId::try_from(s.as_str())
                .map_err(|_| AppError::invalid(DOMAIN_CODE, "非法里程碑 ID"))?,
        ),
        None => None,
    };
    st.svc
        .set_item_milestone(&user, item_id, milestone_id)
        .await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "已关联".to_owned(),
        data: None,
    }))
}

// ---------- 多视图(T-113) ----------

/// 看板列。
#[derive(Debug, Serialize, ToSchema)]
pub struct BoardColumn {
    /// 列状态。
    pub status: String,
    /// 该列工作项(按 board_order 升序)。
    pub items: Vec<ItemResponse>,
    /// 该列总数。
    pub total: i64,
}

/// 看板视图查询参数。
#[derive(Debug, Deserialize, IntoParams)]
pub struct BoardQuery {
    /// 按类型过滤(如 story)。
    pub item_type: Option<String>,
}

/// 看板视图:按状态分组(列序即工作流顺序)。
#[utoipa::path(
    get,
    path = "/api/projects/{key}/board",
    params(BoardQuery, ("key" = String, Path)),
    responses((status = 200))
)]
async fn board_view(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(key): Path<String>,
    Query(q): Query<BoardQuery>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let item_type = q.item_type.as_deref().and_then(ItemType::from_str_opt);
    let view = st.svc.board_view(&user, &key, item_type).await?;
    Ok(Json(ApiResponse::ok(view)))
}

/// 看板拖拽重排请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct BoardReorderRequest {
    /// 变更列表:工作项 ID + 目标状态 + 列内顺序。
    pub items: Vec<BoardPosition>,
}

/// 看板位置。
#[derive(Debug, Deserialize, ToSchema)]
pub struct BoardPosition {
    /// 工作项 ID。
    pub id: String,
    /// 目标状态(拖入的列)。
    pub status: String,
    /// 列内顺序(0 起)。
    pub board_order: i64,
}

/// 看板拖拽重排(跨列拖拽会做工作流校验)。
#[utoipa::path(
    put,
    path = "/api/projects/{key}/board/reorder",
    request_body = BoardReorderRequest,
    params(("key" = String, Path)),
    responses((status = 200), (status = 409))
)]
async fn board_reorder(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(key): Path<String>,
    Json(req): Json<BoardReorderRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let positions = req
        .items
        .iter()
        .map(|p| {
            let id = crate::domain::WorkItemId::try_from(p.id.as_str())
                .map_err(|_| AppError::invalid(DOMAIN_CODE, "非法工作项 ID"))?;
            Ok((id, p.status.clone(), p.board_order))
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    st.svc.reorder_board(&user, &key, &positions).await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "已重排".to_owned(),
        data: None,
    }))
}

/// 树形节点(契约示例;实际载荷为嵌套 JSON)。
#[allow(dead_code)]
#[derive(Debug, Serialize, ToSchema)]
pub struct TreeNode {
    /// 工作项 ID。
    pub id: String,
    /// 展示编号({项目键}-{编号})。
    pub display_key: String,
    /// 类型。
    pub item_type: String,
    /// 标题。
    pub title: String,
    /// 状态。
    pub status: String,
    /// 子节点。
    pub children: Vec<TreeNode>,
}

/// 需求树(epic → story → task)。
#[utoipa::path(
    get,
    path = "/api/projects/{key}/items/tree",
    params(("key" = String, Path)),
    responses((status = 200))
)]
async fn items_tree(
    Extension(user): Extension<CurrentUser>,
    State(st): State<ProjectState>,
    Path(key): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let tree = st.svc.items_tree(&user, &key).await?;
    Ok(Json(ApiResponse::ok(serde_json::Value::Array(tree))))
}
