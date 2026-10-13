//! 应用中心 HTTP 层:应用/环境/部署目标/变量/仓库聚合。
//! 认证经 kernel::TokenAuthenticator 注入 `CurrentUser`(域不依赖 identity)。

use axum::extract::{Path, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use codeplan_common::error::AppError;
use codeplan_common::response::ApiResponse;
use codeplan_kernel::authn::TokenAuthenticator;
use codeplan_kernel::identity::CurrentUser;
use serde::Deserialize;
use std::sync::Arc;
use utoipa::ToSchema;

use crate::DOMAIN_CODE;
use crate::domain::{
    AppAggregate, Application, DeployTarget, EnvKind, EnvVar, Environment, EnvironmentId,
};
use crate::service::AppCenterService;

/// API 状态。
#[derive(Clone)]
pub struct AppState {
    /// 应用中心服务。
    pub svc: Arc<AppCenterService>,
    /// 令牌认证器。
    pub authn: Arc<dyn TokenAuthenticator>,
}

/// 路由。
pub fn routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/apps", get(list_apps).post(create_app))
        .route("/api/apps/{key}", get(app_aggregate))
        .route(
            "/api/apps/{key}/environments",
            get(list_envs).post(create_env),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}",
            axum::routing::delete(delete_env),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}/targets",
            get(list_targets).post(add_target),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}/targets/{target_id}",
            axum::routing::delete(remove_target),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}/vars",
            get(list_vars).post(upsert_var),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}/vars/{var_id}",
            axum::routing::delete(delete_var),
        )
        .route(
            "/api/apps/{key}/repos",
            post(link_repo).delete(unlink_repo),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}/spec",
            get(get_spec).put(apply_spec),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}/spec/versions",
            get(list_spec_versions),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}/config",
            get(get_config).put(apply_config),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}/config/versions",
            get(list_config_versions),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}/config/reveal",
            post(reveal_config),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}/domains",
            get(list_domains).post(bind_domain),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}/domains/auto",
            post(auto_domain),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}/domains/{domain_id}",
            axum::routing::delete(unbind_domain),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}/gateway/nginx",
            get(gateway_nginx),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}/gateway/apisix",
            get(gateway_apisix),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}/gateway/apply",
            post(gateway_apply),
        )
        .route(
            "/api/apps/{key}/environments/{env_id}/certificates",
            get(list_certificates).post(issue_certificate),
        )
        .route("/api/deploy-hosts", get(list_hosts).post(create_host))
        .route(
            "/api/apps/{key}/environments/{env_id}/deployments",
            get(list_deployments).post(deploy),
        )
        .layer(middleware::from_fn_with_state(state, auth_mw))
}

/// 认证中间件。
async fn auth_mw(
    State(st): State<AppState>,
    mut req: axum::extract::Request,
    next: Next,
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

/// 创建应用请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateAppRequest {
    /// 应用键(2~40 位小写字母/数字/连字符)。
    pub key: String,
    /// 名称。
    pub name: String,
    /// 描述。
    #[serde(default)]
    pub description: String,
}

/// 应用响应。
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct AppResponse {
    /// ID。
    pub id: String,
    /// 键。
    pub key: String,
    /// 名称。
    pub name: String,
    /// 描述。
    pub description: String,
    /// 负责人。
    pub owner: Option<String>,
    /// 创建时间。
    pub created_at: String,
}

fn app_response(a: &Application) -> AppResponse {
    AppResponse {
        id: a.id.as_uuid().to_string(),
        key: a.key.clone(),
        name: a.name.clone(),
        description: a.description.clone(),
        owner: a.owner_id.map(|u| u.as_uuid().to_string()),
        created_at: chrono::DateTime::to_rfc3339(&a.created_at),
    }
}

/// 创建环境请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateEnvRequest {
    /// 环境名。
    pub name: String,
    /// 类型:dev/test/staging/prod。
    pub kind: String,
}

/// 环境响应。
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct EnvResponse {
    /// ID。
    pub id: String,
    /// 应用 ID。
    pub app_id: String,
    /// 名称。
    pub name: String,
    /// 类型。
    pub kind: String,
    /// 是否生产。
    pub production: bool,
    /// 创建时间。
    pub created_at: String,
}

fn env_response(e: &Environment) -> EnvResponse {
    EnvResponse {
        id: e.id.as_uuid().to_string(),
        app_id: e.app_id.as_uuid().to_string(),
        name: e.name.clone(),
        kind: e.kind.as_str().to_owned(),
        production: e.kind.is_production(),
        created_at: chrono::DateTime::to_rfc3339(&e.created_at),
    }
}

/// 登记部署目标请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct AddTargetRequest {
    /// 目标名。
    pub name: String,
    /// 主机地址。
    pub host: String,
    /// 端口(缺省 22)。
    #[serde(default = "default_port")]
    pub port: i32,
    /// 标签。
    #[serde(default)]
    pub labels: Vec<String>,
}

fn default_port() -> i32 {
    22
}

/// 部署目标响应。
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct TargetResponse {
    /// ID。
    pub id: String,
    /// 环境 ID。
    pub env_id: String,
    /// 名称。
    pub name: String,
    /// 主机。
    pub host: String,
    /// 端口。
    pub port: i32,
    /// 标签。
    pub labels: Vec<String>,
    /// 登记时间。
    pub created_at: String,
}

fn target_response(t: &DeployTarget) -> TargetResponse {
    TargetResponse {
        id: t.id.as_uuid().to_string(),
        env_id: t.env_id.as_uuid().to_string(),
        name: t.name.clone(),
        host: t.host.clone(),
        port: t.port,
        labels: t.labels.clone(),
        created_at: chrono::DateTime::to_rfc3339(&t.created_at),
    }
}

/// 写入变量请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpsertVarRequest {
    /// 键。
    pub key: String,
    /// 值。
    #[serde(default)]
    pub value: String,
    /// 是否敏感(敏感项列表接口不回显值)。
    #[serde(default)]
    pub is_secret: bool,
}

/// 变量响应(敏感项值脱敏)。
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct VarResponse {
    /// ID。
    pub id: String,
    /// 键。
    pub key: String,
    /// 值(敏感项为 `null`)。
    pub value: Option<String>,
    /// 是否敏感。
    pub is_secret: bool,
}

fn var_response(v: &EnvVar) -> VarResponse {
    VarResponse {
        id: v.id.as_uuid().to_string(),
        key: v.key.clone(),
        value: if v.is_secret { None } else { Some(v.value.clone()) },
        is_secret: v.is_secret,
    }
}

/// 仓库关联请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct LinkRepoRequest {
    /// 仓库 ID。
    pub repo_id: String,
}

/// 环境路径参数。
#[derive(Debug, Deserialize)]
pub struct EnvPath {
    /// 应用键。
    pub key: String,
    /// 环境 ID。
    pub env_id: String,
}

/// 目标路径参数。
#[derive(Debug, Deserialize)]
pub struct TargetPath {
    /// 应用键。
    pub key: String,
    /// 环境 ID。
    pub env_id: String,
    /// 目标 ID。
    pub target_id: String,
}

/// 变量路径参数。
#[derive(Debug, Deserialize)]
pub struct VarPath {
    /// 应用键。
    pub key: String,
    /// 环境 ID。
    pub env_id: String,
    /// 变量 ID。
    pub var_id: String,
}

fn parse_uuid(raw: &str, what: &str) -> Result<uuid::Uuid, AppError> {
    uuid::Uuid::parse_str(raw).map_err(|_| AppError::invalid(DOMAIN_CODE, format!("{what} 非法")))
}

fn parse_env_id(raw: &str) -> Result<EnvironmentId, AppError> {
    Ok(EnvironmentId::from_uuid(parse_uuid(raw, "环境 ID")?))
}

/// 应用列表。
#[utoipa::path(get, path = "/api/apps", responses((status = 200, body = AppResponse)))]
async fn list_apps(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
) -> Result<Json<ApiResponse<Vec<AppResponse>>>, AppError> {
    let apps = st.svc.list_apps(&user).await?;
    Ok(Json(ApiResponse::ok(
        apps.iter().map(app_response).collect(),
    )))
}

/// 创建应用。
#[utoipa::path(
    post,
    path = "/api/apps",
    request_body = CreateAppRequest,
    responses((status = 200, body = AppResponse), (status = 400), (status = 409))
)]
async fn create_app(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Json(req): Json<CreateAppRequest>,
) -> Result<Json<ApiResponse<AppResponse>>, AppError> {
    let app = st
        .svc
        .create_app(&user, &req.key, &req.name, &req.description)
        .await?;
    Ok(Json(ApiResponse::ok(app_response(&app))))
}

/// 应用聚合视图(PAAS-01)。
#[utoipa::path(
    get,
    path = "/api/apps/{key}",
    params(("key" = String, Path, description = "应用键")),
    responses((status = 200, body = AppAggregate), (status = 404))
)]
async fn app_aggregate(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<ApiResponse<AppAggregate>>, AppError> {
    let agg = st.svc.aggregate(&user, &key).await?;
    Ok(Json(ApiResponse::ok(agg)))
}

/// 环境列表。
#[utoipa::path(
    get,
    path = "/api/apps/{key}/environments",
    params(("key" = String, Path)),
    responses((status = 200, body = EnvResponse))
)]
async fn list_envs(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<ApiResponse<Vec<EnvResponse>>>, AppError> {
    let app = st.svc.app_by_key(&user, &key).await?;
    let envs = st.svc.list_envs(&user, app.id).await?;
    Ok(Json(ApiResponse::ok(
        envs.iter().map(env_response).collect(),
    )))
}

/// 创建环境。
#[utoipa::path(
    post,
    path = "/api/apps/{key}/environments",
    request_body = CreateEnvRequest,
    params(("key" = String, Path)),
    responses((status = 200, body = EnvResponse), (status = 400))
)]
async fn create_env(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(key): Path<String>,
    Json(req): Json<CreateEnvRequest>,
) -> Result<Json<ApiResponse<EnvResponse>>, AppError> {
    let kind = EnvKind::parse(&req.kind)
        .ok_or_else(|| AppError::invalid(DOMAIN_CODE, "环境类型须为 dev/test/staging/prod"))?;
    let app = st.svc.app_by_key(&user, &key).await?;
    let env = st.svc.create_env(&user, app.id, &req.name, kind).await?;
    Ok(Json(ApiResponse::ok(env_response(&env))))
}

/// 删除环境(级联目标与变量)。
#[utoipa::path(
    delete,
    path = "/api/apps/{key}/environments/{env_id}",
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200), (status = 404))
)]
async fn delete_env(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let _ = st.svc.app_by_key(&user, &p.key).await?;
    st.svc.delete_env(&user, parse_env_id(&p.env_id)?).await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "已删除".to_owned(),
        data: None,
    }))
}

/// 部署目标列表。
#[utoipa::path(
    get,
    path = "/api/apps/{key}/environments/{env_id}/targets",
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200, body = TargetResponse))
)]
async fn list_targets(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
) -> Result<Json<ApiResponse<Vec<TargetResponse>>>, AppError> {
    let _ = st.svc.app_by_key(&user, &p.key).await?;
    let list = st.svc.list_targets(&user, parse_env_id(&p.env_id)?).await?;
    Ok(Json(ApiResponse::ok(
        list.iter().map(target_response).collect(),
    )))
}

/// 登记部署目标。
#[utoipa::path(
    post,
    path = "/api/apps/{key}/environments/{env_id}/targets",
    request_body = AddTargetRequest,
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200, body = TargetResponse), (status = 400))
)]
async fn add_target(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
    Json(req): Json<AddTargetRequest>,
) -> Result<Json<ApiResponse<TargetResponse>>, AppError> {
    let _ = st.svc.app_by_key(&user, &p.key).await?;
    let t = st
        .svc
        .add_target(
            &user,
            parse_env_id(&p.env_id)?,
            &req.name,
            &req.host,
            req.port,
            req.labels,
        )
        .await?;
    Ok(Json(ApiResponse::ok(target_response(&t))))
}

/// 移除部署目标。
#[utoipa::path(
    delete,
    path = "/api/apps/{key}/environments/{env_id}/targets/{target_id}",
    params(("key" = String, Path), ("env_id" = String, Path), ("target_id" = String, Path)),
    responses((status = 200), (status = 404))
)]
async fn remove_target(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<TargetPath>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let _ = st.svc.app_by_key(&user, &p.key).await?;
    let tid = crate::domain::DeployTargetId::from_uuid(parse_uuid(&p.target_id, "目标 ID")?);
    st.svc
        .remove_target(&user, parse_env_id(&p.env_id)?, tid)
        .await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "已移除".to_owned(),
        data: None,
    }))
}

/// 环境变量列表(敏感项不回显值)。
#[utoipa::path(
    get,
    path = "/api/apps/{key}/environments/{env_id}/vars",
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200, body = VarResponse))
)]
async fn list_vars(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
) -> Result<Json<ApiResponse<Vec<VarResponse>>>, AppError> {
    let _ = st.svc.app_by_key(&user, &p.key).await?;
    let list = st.svc.list_vars(&user, parse_env_id(&p.env_id)?).await?;
    Ok(Json(ApiResponse::ok(list.iter().map(var_response).collect())))
}

/// 写入/覆盖环境变量。
#[utoipa::path(
    post,
    path = "/api/apps/{key}/environments/{env_id}/vars",
    request_body = UpsertVarRequest,
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200, body = VarResponse), (status = 400))
)]
async fn upsert_var(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
    Json(req): Json<UpsertVarRequest>,
) -> Result<Json<ApiResponse<VarResponse>>, AppError> {
    let _ = st.svc.app_by_key(&user, &p.key).await?;
    let v = st
        .svc
        .upsert_var(
            &user,
            parse_env_id(&p.env_id)?,
            &req.key,
            &req.value,
            req.is_secret,
        )
        .await?;
    Ok(Json(ApiResponse::ok(var_response(&v))))
}

/// 删除环境变量。
#[utoipa::path(
    delete,
    path = "/api/apps/{key}/environments/{env_id}/vars/{var_id}",
    params(("key" = String, Path), ("env_id" = String, Path), ("var_id" = String, Path)),
    responses((status = 200), (status = 404))
)]
async fn delete_var(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<VarPath>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let _ = st.svc.app_by_key(&user, &p.key).await?;
    let vid = crate::domain::EnvVarId::from_uuid(parse_uuid(&p.var_id, "变量 ID")?);
    st.svc
        .delete_var(&user, parse_env_id(&p.env_id)?, vid)
        .await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "已删除".to_owned(),
        data: None,
    }))
}

/// 关联仓库(聚合视图数据源)。
#[utoipa::path(
    post,
    path = "/api/apps/{key}/repos",
    request_body = LinkRepoRequest,
    params(("key" = String, Path)),
    responses((status = 200), (status = 404))
)]
async fn link_repo(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(key): Path<String>,
    Json(req): Json<LinkRepoRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let app = st.svc.app_by_key(&user, &key).await?;
    st.svc
        .link_repo(&user, app.id, parse_uuid(&req.repo_id, "仓库 ID")?)
        .await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "已关联".to_owned(),
        data: None,
    }))
}

/// 解除仓库关联。
#[utoipa::path(
    delete,
    path = "/api/apps/{key}/repos",
    request_body = LinkRepoRequest,
    params(("key" = String, Path)),
    responses((status = 200), (status = 404))
)]
async fn unlink_repo(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(key): Path<String>,
    Json(req): Json<LinkRepoRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let app = st.svc.app_by_key(&user, &key).await?;
    st.svc
        .unlink_repo(&user, app.id, parse_uuid(&req.repo_id, "仓库 ID")?)
        .await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "已解除".to_owned(),
        data: None,
    }))
}

// ---------- 服务规格(T-312 / PAAS-03) ----------

/// 提交规格请求(规格即代码:JSON 或 YAML 文本)。
#[derive(Debug, Deserialize, ToSchema)]
pub struct ApplySpecRequest {
    /// 声明文本(JSON/YAML);未知字段视为错误。
    pub spec: String,
}

/// 规格响应。
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct SpecResponse {
    /// 版本号。
    pub version: i32,
    /// 规格。
    pub spec: crate::domain::ServiceSpec,
    /// 原始声明文本。
    pub source: String,
    /// 提交人。
    pub created_by: Option<String>,
    /// 提交时间。
    pub created_at: String,
}

fn spec_response(v: &crate::domain::SpecVersion) -> SpecResponse {
    SpecResponse {
        version: v.version,
        spec: v.spec.clone(),
        source: v.source.clone(),
        created_by: v.created_by.clone(),
        created_at: v.created_at.clone(),
    }
}

/// 当前规格(未提交过返回 `data: null`)。
#[utoipa::path(
    get,
    path = "/api/apps/{key}/environments/{env_id}/spec",
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200, body = SpecResponse))
)]
async fn get_spec(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
) -> Result<Json<ApiResponse<Option<SpecResponse>>>, AppError> {
    let cur = st
        .svc
        .current_spec(&user, &p.key, parse_env_id(&p.env_id)?)
        .await?;
    Ok(Json(ApiResponse::ok(cur.as_ref().map(spec_response))))
}

/// 提交规格(追加新版本并置为当前)。
#[utoipa::path(
    put,
    path = "/api/apps/{key}/environments/{env_id}/spec",
    request_body = ApplySpecRequest,
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200, body = SpecResponse), (status = 400))
)]
async fn apply_spec(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
    Json(req): Json<ApplySpecRequest>,
) -> Result<Json<ApiResponse<SpecResponse>>, AppError> {
    let v = st
        .svc
        .apply_spec(&user, &p.key, parse_env_id(&p.env_id)?, &req.spec)
        .await?;
    Ok(Json(ApiResponse::ok(spec_response(&v))))
}

/// 规格版本历史(新→旧)。
#[utoipa::path(
    get,
    path = "/api/apps/{key}/environments/{env_id}/spec/versions",
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200, body = SpecResponse))
)]
async fn list_spec_versions(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
) -> Result<Json<ApiResponse<Vec<SpecResponse>>>, AppError> {
    let list = st
        .svc
        .list_spec_versions(&user, &p.key, parse_env_id(&p.env_id)?)
        .await?;
    Ok(Json(ApiResponse::ok(
        list.iter().map(spec_response).collect(),
    )))
}

// ---------- 配置与密钥(T-317 / PAAS-06) ----------

/// 提交配置请求(YAML/JSON 映射;敏感项写 `{value, secret: true}`)。
#[derive(Debug, Deserialize, ToSchema)]
pub struct ApplyConfigRequest {
    /// 声明文本。
    pub config: String,
}

/// 单键解密请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct RevealRequest {
    /// 配置键。
    pub key: String,
}

/// 单键解密响应。
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct RevealResponse {
    /// 配置键。
    pub key: String,
    /// 明文值。
    pub value: String,
    /// 是否来自密文(敏感项)。
    pub was_secret: bool,
}

/// 当前配置(敏感项不回显值;未提交过返回 `data: null`)。
#[utoipa::path(
    get,
    path = "/api/apps/{key}/environments/{env_id}/config",
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200))
)]
async fn get_config(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
) -> Result<Json<ApiResponse<Option<crate::domain::ConfigVersion>>>, AppError> {
    let cur = st
        .svc
        .current_config(&user, &p.key, parse_env_id(&p.env_id)?)
        .await?;
    Ok(Json(ApiResponse::ok(cur)))
}

/// 提交配置(敏感项加密落库,追加新版本)。
#[utoipa::path(
    put,
    path = "/api/apps/{key}/environments/{env_id}/config",
    request_body = ApplyConfigRequest,
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200), (status = 400))
)]
async fn apply_config(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
    Json(req): Json<ApplyConfigRequest>,
) -> Result<Json<ApiResponse<crate::domain::ConfigVersion>>, AppError> {
    let v = st
        .svc
        .apply_config(&user, &p.key, parse_env_id(&p.env_id)?, &req.config)
        .await?;
    Ok(Json(ApiResponse::ok(v)))
}

/// 配置版本历史(新→旧;敏感项不回显值)。
#[utoipa::path(
    get,
    path = "/api/apps/{key}/environments/{env_id}/config/versions",
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200))
)]
async fn list_config_versions(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
) -> Result<Json<ApiResponse<Vec<crate::domain::ConfigVersion>>>, AppError> {
    let list = st
        .svc
        .list_config_versions(&user, &p.key, parse_env_id(&p.env_id)?)
        .await?;
    Ok(Json(ApiResponse::ok(list)))
}

/// 解密单个配置项(需写权限;动作留审计,不记录值)。
#[utoipa::path(
    post,
    path = "/api/apps/{key}/environments/{env_id}/config/reveal",
    request_body = RevealRequest,
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200, body = RevealResponse), (status = 403), (status = 404))
)]
async fn reveal_config(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
    Json(req): Json<RevealRequest>,
) -> Result<Json<ApiResponse<RevealResponse>>, AppError> {
    let env_id = parse_env_id(&p.env_id)?;
    let value = st
        .svc
        .reveal_config_key(&user, &p.key, env_id, req.key.trim())
        .await?;
    Ok(Json(ApiResponse::ok(RevealResponse {
        key: req.key.trim().to_owned(),
        value,
        was_secret: true,
    })))
}

// ---------- 域名绑定与网关路由(T-315 / PAAS-05) ----------

/// 绑定域名请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct BindDomainRequest {
    /// 主机名(多段域名)。
    pub host: String,
    /// 是否启用 TLS(缺省 true)。
    #[serde(default = "default_true")]
    pub tls_enabled: bool,
}

fn default_true() -> bool {
    true
}

/// 域名响应。
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct DomainResponse {
    /// 绑定 ID。
    pub id: String,
    /// 环境 ID。
    pub env_id: String,
    /// 主机名。
    pub host: String,
    /// 来源:auto/custom。
    pub kind: String,
    /// 是否启用 TLS。
    pub tls_enabled: bool,
    /// 证书引用。
    pub cert_ref: String,
    /// 绑定时间。
    pub created_at: String,
}

fn domain_response(d: &crate::domain::AppDomain) -> DomainResponse {
    DomainResponse {
        id: d.id.as_uuid().to_string(),
        env_id: d.env_id.as_uuid().to_string(),
        host: d.host.clone(),
        kind: d.kind.as_str().to_owned(),
        tls_enabled: d.tls_enabled,
        cert_ref: d.cert_ref.clone(),
        created_at: chrono::DateTime::to_rfc3339(&d.created_at),
    }
}

/// 域名绑定路径参数。
#[derive(Debug, Deserialize)]
pub struct DomainPath {
    /// 应用键。
    pub key: String,
    /// 环境 ID。
    pub env_id: String,
    /// 绑定 ID。
    pub domain_id: String,
}

/// 域名列表。
#[utoipa::path(
    get,
    path = "/api/apps/{key}/environments/{env_id}/domains",
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200, body = DomainResponse))
)]
async fn list_domains(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
) -> Result<Json<ApiResponse<Vec<DomainResponse>>>, AppError> {
    let list = st
        .svc
        .list_domains(&user, &p.key, parse_env_id(&p.env_id)?)
        .await?;
    Ok(Json(ApiResponse::ok(
        list.iter().map(domain_response).collect(),
    )))
}

/// 绑定自定义域名。
#[utoipa::path(
    post,
    path = "/api/apps/{key}/environments/{env_id}/domains",
    request_body = BindDomainRequest,
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200, body = DomainResponse), (status = 400), (status = 409))
)]
async fn bind_domain(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
    Json(req): Json<BindDomainRequest>,
) -> Result<Json<ApiResponse<DomainResponse>>, AppError> {
    let d = st
        .svc
        .bind_domain(
            &user,
            &p.key,
            parse_env_id(&p.env_id)?,
            &req.host,
            req.tls_enabled,
        )
        .await?;
    Ok(Json(ApiResponse::ok(domain_response(&d))))
}

/// 自动分配域名(幂等)。
#[utoipa::path(
    post,
    path = "/api/apps/{key}/environments/{env_id}/domains/auto",
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200, body = DomainResponse))
)]
async fn auto_domain(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
) -> Result<Json<ApiResponse<DomainResponse>>, AppError> {
    let d = st
        .svc
        .auto_assign_domain(&user, &p.key, parse_env_id(&p.env_id)?)
        .await?;
    Ok(Json(ApiResponse::ok(domain_response(&d))))
}

/// 解绑域名。
#[utoipa::path(
    delete,
    path = "/api/apps/{key}/environments/{env_id}/domains/{domain_id}",
    params(("key" = String, Path), ("env_id" = String, Path), ("domain_id" = String, Path)),
    responses((status = 200), (status = 404))
)]
async fn unbind_domain(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<DomainPath>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let did = crate::domain::DomainId::from_uuid(parse_uuid(&p.domain_id, "绑定 ID")?);
    st.svc
        .unbind_domain(&user, &p.key, parse_env_id(&p.env_id)?, did)
        .await?;
    Ok(Json(ApiResponse {
        code: "OK".to_owned(),
        message: "已解绑".to_owned(),
        data: None,
    }))
}

/// 渲染 Nginx 站点配置(纯文本,便于直接落地)。
#[utoipa::path(
    get,
    path = "/api/apps/{key}/environments/{env_id}/gateway/nginx",
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200))
)]
async fn gateway_nginx(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
) -> Result<Json<ApiResponse<String>>, AppError> {
    let cfg = st
        .svc
        .gateway_nginx(&user, &p.key, parse_env_id(&p.env_id)?)
        .await?;
    Ok(Json(ApiResponse::ok(cfg)))
}

/// 渲染 APISIX 路由(Admin API 适配)。
#[utoipa::path(
    get,
    path = "/api/apps/{key}/environments/{env_id}/gateway/apisix",
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200))
)]
async fn gateway_apisix(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let (id, body) = st
        .svc
        .gateway_apisix(&user, &p.key, parse_env_id(&p.env_id)?)
        .await?;
    Ok(Json(ApiResponse::ok(serde_json::json!({
        "route_id": id,
        "admin_path": format!("/apisix/admin/routes/{id}"),
        "body": body,
    }))))
}

/// 落地 Nginx 配置到网关配置目录。
#[utoipa::path(
    post,
    path = "/api/apps/{key}/environments/{env_id}/gateway/apply",
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200))
)]
async fn gateway_apply(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let (path, bytes) = st
        .svc
        .apply_gateway(&user, &p.key, parse_env_id(&p.env_id)?)
        .await?;
    Ok(Json(ApiResponse::ok(serde_json::json!({
        "path": path,
        "bytes": bytes,
    }))))
}

// ---------- 证书(T-316 / PAAS-07) ----------

/// 证书响应。
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct CertResponse {
    /// 证书 ID。
    pub id: String,
    /// 环境 ID。
    pub env_id: String,
    /// 证书引用(网关按 `{cert_ref}.crt/.key` 读取)。
    pub cert_ref: String,
    /// 覆盖域名。
    pub domains: Vec<String>,
    /// 签发方。
    pub provider: String,
    /// 状态。
    pub status: String,
    /// 到期时间。
    pub not_after: Option<String>,
    /// 签发时间。
    pub issued_at: String,
}

fn cert_response(c: &crate::domain::Certificate) -> CertResponse {
    CertResponse {
        id: c.id.as_uuid().to_string(),
        env_id: c.env_id.as_uuid().to_string(),
        cert_ref: c.cert_ref.clone(),
        domains: c.domains.clone(),
        provider: c.provider.clone(),
        status: c.status.clone(),
        not_after: c.not_after.map(|d| d.to_rfc3339()),
        issued_at: chrono::DateTime::to_rfc3339(&c.issued_at),
    }
}

/// 证书列表。
#[utoipa::path(
    get,
    path = "/api/apps/{key}/environments/{env_id}/certificates",
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200, body = CertResponse))
)]
async fn list_certificates(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
) -> Result<Json<ApiResponse<Vec<CertResponse>>>, AppError> {
    let list = st
        .svc
        .list_certificates(&user, &p.key, parse_env_id(&p.env_id)?)
        .await?;
    Ok(Json(ApiResponse::ok(
        list.iter().map(cert_response).collect(),
    )))
}

/// 申请/续期证书(DNS-01;落盘后回填域名 `cert_ref`,网关随即启用 TLS)。
#[utoipa::path(
    post,
    path = "/api/apps/{key}/environments/{env_id}/certificates",
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200, body = CertResponse), (status = 400))
)]
async fn issue_certificate(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
) -> Result<Json<ApiResponse<CertResponse>>, AppError> {
    let c = st
        .svc
        .issue_certificate(&user, &p.key, parse_env_id(&p.env_id)?)
        .await?;
    Ok(Json(ApiResponse::ok(cert_response(&c))))
}

// ---------- 部署主机(T-311 / PAAS-03) ----------

/// 登记主机请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateHostRequest {
    /// 平台侧登记名(唯一)。
    pub name: String,
    /// 标签。
    #[serde(default)]
    pub labels: Vec<String>,
}

/// 主机响应。
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct HostResponse {
    /// 主机 ID。
    pub id: String,
    /// 登记名。
    pub name: String,
    /// 是否已被 agent 注册。
    pub registered: bool,
    /// agent 上报主机名。
    pub hostname: String,
    /// agent 版本。
    pub agent_version: String,
    /// 标签。
    pub labels: Vec<String>,
    /// 在线状态。
    pub status: String,
    /// 最后心跳时间。
    pub last_heartbeat_at: Option<String>,
    /// 登记时间。
    pub created_at: String,
}

fn host_response(h: &crate::domain::DeployHost) -> HostResponse {
    HostResponse {
        id: h.id.as_uuid().to_string(),
        name: h.name.clone(),
        registered: h.registered,
        hostname: h.hostname.clone(),
        agent_version: h.agent_version.clone(),
        labels: h.labels.clone(),
        status: h.status.as_str().to_owned(),
        last_heartbeat_at: h.last_heartbeat_at.map(|d| d.to_rfc3339()),
        created_at: chrono::DateTime::to_rfc3339(&h.created_at),
    }
}

/// 登记主机(返回**仅此一次**可见的注册令牌)。
#[utoipa::path(
    post,
    path = "/api/deploy-hosts",
    request_body = CreateHostRequest,
    responses((status = 200), (status = 403), (status = 409))
)]
async fn create_host(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Json(req): Json<CreateHostRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let (host, token) = st
        .svc
        .create_host(&user, &req.name, req.labels)
        .await?;
    Ok(Json(ApiResponse::ok(serde_json::json!({
        "host": host_response(&host),
        "register_token": token,
        "agent_hint": "CODEPLAN_AGENT_SERVER=<平台地址> CODEPLAN_AGENT_TOKEN=<register_token> ./codeplan-agent",
    }))))
}

/// 主机列表(含实时在线状态)。
#[utoipa::path(get, path = "/api/deploy-hosts", responses((status = 200, body = HostResponse)))]
async fn list_hosts(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
) -> Result<Json<ApiResponse<Vec<HostResponse>>>, AppError> {
    let hosts = st.svc.list_hosts(&user).await?;
    Ok(Json(ApiResponse::ok(
        hosts.iter().map(host_response).collect(),
    )))
}

// ---------- 部署执行(T-313 / PAAS-04) ----------

/// 部署记录响应。
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct DeploymentResponse {
    /// 部署 ID。
    pub id: String,
    /// 环境 ID。
    pub env_id: String,
    /// 应用键。
    pub app_key: String,
    /// 规格版本。
    pub spec_version: Option<i32>,
    /// 状态。
    pub status: String,
    /// 目标主机数。
    pub target_count: usize,
    /// 各主机回执。
    pub results: std::collections::BTreeMap<String, String>,
    /// 说明。
    pub detail: String,
    /// 创建时间。
    pub created_at: String,
    /// 结束时间。
    pub finished_at: Option<String>,
}

fn deployment_response(d: &crate::domain::Deployment) -> DeploymentResponse {
    DeploymentResponse {
        id: d.id.as_uuid().to_string(),
        env_id: d.env_id.as_uuid().to_string(),
        app_key: d.app_key.clone(),
        spec_version: d.spec_version,
        status: d.status.as_str().to_owned(),
        target_count: d.target_hosts.len(),
        results: d.results.clone(),
        detail: d.detail.clone(),
        created_at: chrono::DateTime::to_rfc3339(&d.created_at),
        finished_at: d.finished_at.map(|t| t.to_rfc3339()),
    }
}

/// 触发一键部署。
#[utoipa::path(
    post,
    path = "/api/apps/{key}/environments/{env_id}/deployments",
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200, body = DeploymentResponse), (status = 400))
)]
async fn deploy(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
) -> Result<Json<ApiResponse<DeploymentResponse>>, AppError> {
    let d = st
        .svc
        .deploy(&user, &p.key, parse_env_id(&p.env_id)?)
        .await?;
    Ok(Json(ApiResponse::ok(deployment_response(&d))))
}

/// 部署记录列表。
#[utoipa::path(
    get,
    path = "/api/apps/{key}/environments/{env_id}/deployments",
    params(("key" = String, Path), ("env_id" = String, Path)),
    responses((status = 200, body = DeploymentResponse))
)]
async fn list_deployments(
    Extension(user): Extension<CurrentUser>,
    State(st): State<AppState>,
    Path(p): Path<EnvPath>,
) -> Result<Json<ApiResponse<Vec<DeploymentResponse>>>, AppError> {
    let list = st
        .svc
        .list_deployments(&user, &p.key, parse_env_id(&p.env_id)?, 20)
        .await?;
    Ok(Json(ApiResponse::ok(
        list.iter().map(deployment_response).collect(),
    )))
}
