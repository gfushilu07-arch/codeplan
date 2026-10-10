//! HTTP 层:站内通知路由(接收人本人操作)。

use axum::extract::{Query, State};
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
use crate::domain::NotificationStore;

/// api 层状态。
#[derive(Clone)]
pub struct NotifyState {
    /// 通知存储。
    pub store: Arc<dyn NotificationStore>,
    /// 令牌认证器。
    pub authn: Arc<dyn TokenAuthenticator>,
}

/// 路由。
pub fn routes(state: NotifyState) -> Router<NotifyState> {
    Router::new()
        .route("/api/notifications", get(list_notifications))
        .route("/api/notifications/unread-count", get(unread_count))
        .route("/api/notifications/read-all", post(read_all))
        .route("/api/notifications/{id}/read", post(mark_read))
        .layer(middleware::from_fn_with_state(state, auth_mw))
}

/// 认证中间件。
async fn auth_mw(
    State(st): State<NotifyState>,
    mut req: axum::extract::Request,
    next: middleware::Next,
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

/// 通知响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct NotificationResponse {
    /// 通知 ID。
    pub id: String,
    /// 标题。
    pub title: String,
    /// 正文。
    pub body: String,
    /// 跳转路径。
    pub link: Option<String>,
    /// 渠道。
    pub channel: String,
    /// 是否已读。
    pub read: bool,
    /// 创建时间(RFC 3339)。
    pub created_at: String,
}

/// 列表查询参数。
#[derive(Debug, Deserialize, IntoParams)]
pub struct ListQuery {
    /// 仅看未读。
    #[serde(default)]
    pub unread_only: bool,
    /// 上限(1~200,默认 50)。
    pub limit: Option<i64>,
}

/// 未读数响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct UnreadCountResponse {
    /// 未读数。
    pub count: i64,
}

async fn list_notifications(
    Extension(user): Extension<CurrentUser>,
    State(st): State<NotifyState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<ApiResponse<Vec<NotificationResponse>>>, AppError> {
    let list = st
        .store
        .list_for(user.id, q.unread_only, q.limit.unwrap_or(50))
        .await?;
    Ok(Json(ApiResponse::ok(
        list.into_iter()
            .map(|n| NotificationResponse {
                id: n.id.to_string(),
                title: n.title,
                body: n.body,
                link: n.link,
                channel: n.channel,
                read: n.read_at.is_some(),
                created_at: chrono::DateTime::to_rfc3339(&n.created_at),
            })
            .collect(),
    )))
}

async fn unread_count(
    Extension(user): Extension<CurrentUser>,
    State(st): State<NotifyState>,
) -> Result<Json<ApiResponse<UnreadCountResponse>>, AppError> {
    let count = st.store.unread_count(user.id).await?;
    Ok(Json(ApiResponse::ok(UnreadCountResponse { count })))
}

async fn mark_read(
    Extension(user): Extension<CurrentUser>,
    State(st): State<NotifyState>,
    axum::extract::Path(id): axum::extract::Path<uuid::Uuid>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    st.store.mark_read(id, user.id).await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "已读".to_owned(),
        data: None,
    }))
}

async fn read_all(
    Extension(user): Extension<CurrentUser>,
    State(st): State<NotifyState>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    st.store.mark_all_read(user.id).await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "全部已读".to_owned(),
        data: None,
    }))
}
