//! Release/Tag API(T-208)。

use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use codeplan_common::error::AppError;
use codeplan_kernel::authn::TokenAuthenticator;
use codeplan_kernel::identity::CurrentUser;
use serde::Deserialize;

use crate::DOMAIN_CODE;
use crate::release_service::ReleaseService;

/// API 状态。
#[derive(Clone)]
pub struct ReleaseState {
    /// Release 服务。
    pub svc: std::sync::Arc<ReleaseService>,
    /// 令牌认证器。
    pub authn: std::sync::Arc<dyn TokenAuthenticator>,
}

/// 路由。
pub fn routes(state: ReleaseState) -> Router<ReleaseState> {
    Router::new()
        .route(
            "/api/repos/{owner}/{key}/tags",
            post(create_tag).get(list_tags),
        )
        .route(
            "/api/repos/{owner}/{key}/tags/{name}",
            axum::routing::delete(delete_tag),
        )
        .route(
            "/api/repos/{owner}/{key}/releases",
            post(publish_release).get(list_releases),
        )
        .route(
            "/api/repos/{owner}/{key}/releases/{tag}",
            get(get_release).put(update_release).delete(delete_release),
        )
        .route(
            "/api/repos/{owner}/{key}/releases/{tag}/assets/{filename}",
            get(download_asset).put(upload_asset).delete(delete_asset),
        )
        .layer(middleware::from_fn_with_state(state, auth_mw))
}

async fn auth_mw(
    State(st): State<ReleaseState>,
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

fn full_name(owner: &str, key: &str) -> String {
    format!("{owner}/{key}")
}

fn release_json(
    r: &crate::release::Release,
    assets: &[crate::release::ReleaseAsset],
) -> serde_json::Value {
    serde_json::json!({
        "tag": r.tag_name,
        "name": r.name,
        "body": r.body,
        "notes_auto": r.notes_auto,
        "author": r.author.as_uuid(),
        "assets": assets.iter().map(|a| serde_json::json!({
            "filename": a.filename,
            "size": a.size,
        })).collect::<Vec<_>>(),
        "created_at": r.created_at.to_rfc3339(),
    })
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct CreateTagRequest {
    /// 标签名。
    pub name: String,
    /// 目标引用(缺省默认分支)。
    pub target: Option<String>,
    /// 附注信息。
    #[serde(default)]
    pub message: String,
}

async fn create_tag(
    State(st): State<ReleaseState>,
    Path((owner, key)): Path<(String, String)>,
    req: axum::Extension<CurrentUser>,
    Json(body): Json<CreateTagRequest>,
) -> Result<Response, AppError> {
    st.svc
        .create_tag(
            &req.0,
            &full_name(&owner, &key),
            &body.name,
            body.target.as_deref(),
            &body.message,
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"code": "OK", "data": {"name": body.name}})),
    )
        .into_response())
}

async fn list_tags(
    State(st): State<ReleaseState>,
    Path((owner, key)): Path<(String, String)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let tags = st.svc.list_tags(&req.0, &full_name(&owner, &key)).await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": tags})).into_response())
}

async fn delete_tag(
    State(st): State<ReleaseState>,
    Path((owner, key, name)): Path<(String, String, String)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    st.svc
        .delete_tag(&req.0, &full_name(&owner, &key), &name)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK"})).into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct PublishReleaseRequest {
    /// 标签名(必须已存在)。
    pub tag_name: String,
    /// 标题(缺省 = 标签名)。
    #[serde(default)]
    pub name: String,
    /// Notes(缺省自动生成)。
    pub body: Option<String>,
    /// 上一标签(缺省自动探测)。
    pub previous_tag: Option<String>,
}

async fn publish_release(
    State(st): State<ReleaseState>,
    Path((owner, key)): Path<(String, String)>,
    req: axum::Extension<CurrentUser>,
    Json(body): Json<PublishReleaseRequest>,
) -> Result<Response, AppError> {
    let release = st
        .svc
        .publish(
            &req.0,
            &full_name(&owner, &key),
            &body.tag_name,
            &body.name,
            body.body.as_deref(),
            body.previous_tag.as_deref(),
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"code": "OK", "data": release_json(&release, &[])})),
    )
        .into_response())
}

async fn list_releases(
    State(st): State<ReleaseState>,
    Path((owner, key)): Path<(String, String)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let releases = st.svc.list(&req.0, &full_name(&owner, &key)).await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": releases
        .iter()
        .map(|(r, a)| release_json(r, a))
        .collect::<Vec<_>>()}))
    .into_response())
}

async fn get_release(
    State(st): State<ReleaseState>,
    Path((owner, key, tag)): Path<(String, String, String)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let (release, assets) = st.svc.get(&req.0, &full_name(&owner, &key), &tag).await?;
    Ok(
        Json(serde_json::json!({"code": "OK", "data": release_json(&release, &assets)}))
            .into_response(),
    )
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct UpdateReleaseRequest {
    /// 标题。
    pub name: Option<String>,
    /// Notes。
    pub body: Option<String>,
}

async fn update_release(
    State(st): State<ReleaseState>,
    Path((owner, key, tag)): Path<(String, String, String)>,
    req: axum::Extension<CurrentUser>,
    Json(body): Json<UpdateReleaseRequest>,
) -> Result<Response, AppError> {
    let release = st
        .svc
        .update(
            &req.0,
            &full_name(&owner, &key),
            &tag,
            body.name.as_deref(),
            body.body.as_deref(),
        )
        .await?;
    Ok(
        Json(serde_json::json!({"code": "OK", "data": release_json(&release, &[])}))
            .into_response(),
    )
}

async fn delete_release(
    State(st): State<ReleaseState>,
    Path((owner, key, tag)): Path<(String, String, String)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    st.svc
        .delete(&req.0, &full_name(&owner, &key), &tag)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK"})).into_response())
}

async fn upload_asset(
    State(st): State<ReleaseState>,
    Path((owner, key, tag, filename)): Path<(String, String, String, String)>,
    req: axum::Extension<CurrentUser>,
    body: axum::body::Body,
) -> Result<Response, AppError> {
    let data = axum::body::to_bytes(body, crate::release_service::MAX_ASSET_SIZE)
        .await
        .map_err(|e| AppError::invalid(DOMAIN_CODE, format!("附件读取失败: {e}")))?;
    let asset = st
        .svc
        .upload_asset(
            &req.0,
            &full_name(&owner, &key),
            &tag,
            &filename,
            data.to_vec(),
        )
        .await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": {
        "filename": asset.filename,
        "size": asset.size,
    }}))
    .into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct AssetDownloadQuery {
    /// 绕过缓存的下载令牌占位。
    #[serde(default)]
    pub _t: Option<String>,
}

async fn download_asset(
    State(st): State<ReleaseState>,
    Path((owner, key, tag, filename)): Path<(String, String, String, String)>,
    req: axum::Extension<CurrentUser>,
    Query(_q): Query<AssetDownloadQuery>,
) -> Result<Response, AppError> {
    let data = st
        .svc
        .download_asset(&req.0, &full_name(&owner, &key), &tag, &filename)
        .await?;
    Ok((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        [(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )],
        data,
    )
        .into_response())
}

async fn delete_asset(
    State(st): State<ReleaseState>,
    Path((owner, key, tag, filename)): Path<(String, String, String, String)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    st.svc
        .delete_asset(&req.0, &full_name(&owner, &key), &tag, &filename)
        .await?;
    Ok(Json(serde_json::json!({"code": "OK"})).into_response())
}
