//! 搜索 API(T-213)。

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use codeplan_common::error::AppError;
use codeplan_kernel::authn::TokenAuthenticator;
use codeplan_kernel::identity::CurrentUser;
use serde::Deserialize;

use crate::index::{DocKind, SearchService};

/// API 状态。
#[derive(Clone)]
pub struct SearchState {
    /// 搜索服务。
    pub svc: Arc<SearchService>,
    /// 令牌认证器。
    pub authn: Arc<dyn TokenAuthenticator>,
}

/// 路由。
pub fn routes(state: SearchState) -> Router<SearchState> {
    Router::new()
        .route("/api/search", get(search))
        .layer(middleware::from_fn_with_state(state, auth_mw))
}

async fn auth_mw(
    State(st): State<SearchState>,
    mut req: axum::extract::Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::unauthenticated("SR"))?;
    let user = st
        .authn
        .verify(token)
        .await
        .map_err(|_| AppError::unauthenticated("SR"))?;
    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    /// 查询文本。
    pub q: String,
    /// 类型过滤(workitem/wiki)。
    pub kind: Option<String>,
    /// 条数上限(缺省 20,≤50)。
    pub limit: Option<usize>,
}

async fn search(
    State(st): State<SearchState>,
    req: axum::Extension<CurrentUser>,
    Query(q): Query<SearchQuery>,
) -> Result<Response, AppError> {
    let _ = req;
    let kind = q.kind.as_deref().and_then(DocKind::parse);
    if q.kind.is_some() && kind.is_none() {
        return Err(AppError::invalid("SR", "kind 取值非法"));
    }
    let hits = st.svc.search(&q.q, kind, q.limit.unwrap_or(20).min(50))?;
    Ok(Json(serde_json::json!({"code": "OK", "data": hits})).into_response())
}
