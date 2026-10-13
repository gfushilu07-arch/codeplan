//! Workflow 校验 API(T-301):上传 YAML → 解析结果或逐条错误。

use axum::extract::{Path, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use codeplan_common::error::AppError;
use codeplan_kernel::authn::TokenAuthenticator;
use codeplan_kernel::identity::CurrentUser;
use serde::Deserialize;

/// API 状态。
#[derive(Clone)]
pub struct CiState {
    /// 令牌认证器。
    pub authn: std::sync::Arc<dyn TokenAuthenticator>,
    /// Runner 池(T-302)。
    pub runners: std::sync::Arc<crate::runner::RunnerPool>,
    /// 执行引擎(T-303)。
    pub engine: Option<std::sync::Arc<crate::engine::Engine>>,
    /// 存储(T-303)。
    pub store: std::sync::Arc<crate::store::CiStore>,
    /// PG(只读查询,可空则用 engine.store.pool)。
    pub pool: Option<sqlx::PgPool>,
}

/// 路由。
pub fn routes(state: CiState) -> Router<CiState> {
    Router::new()
        .route("/api/ci/workflows/validate", post(validate))
        .route("/api/ci/runners", post(register_runner).get(list_runners))
        .route("/api/ci/runners/{id}/heartbeat", post(runner_heartbeat))
        .route(
            "/api/repos/{owner}/{key}/pipelines",
            post(save_pipeline).get(list_pipelines),
        )
        .route(
            "/api/repos/{owner}/{key}/pipelines/{id}",
            axum::routing::delete(delete_pipeline),
        )
        .route(
            "/api/repos/{owner}/{key}/pipelines/{id}/dispatch",
            post(dispatch_pipeline),
        )
        .route("/api/repos/{owner}/{key}/runs", get(list_runs))
        .route("/api/ci/runs/{id}", get(run_detail))
        .route("/api/ci/runs/{id}/logs/stream", get(run_logs_sse))
        .layer(middleware::from_fn_with_state(state, auth_mw))
}

async fn auth_mw(
    State(st): State<CiState>,
    mut req: axum::extract::Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::unauthenticated("CI"))?;
    let user: CurrentUser = st
        .authn
        .verify(token)
        .await
        .map_err(|_| AppError::unauthenticated("CI"))?;
    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

#[derive(Debug, Deserialize)]
struct ValidateRequest {
    /// workflow YAML 全文。
    pub yaml: String,
}

async fn validate(
    axum::Extension(_user): axum::Extension<CurrentUser>,
    Json(body): Json<ValidateRequest>,
) -> Result<Response, AppError> {
    match crate::workflow::parse_workflow(&body.yaml) {
        Ok(wf) => Ok(Json(serde_json::json!({
            "code": "OK",
            "data": {
                "valid": true,
                "workflow": wf,
                "matrix_combos": wf.jobs.iter().map(|j| serde_json::json!({
                    "job": j.id,
                    "combos": crate::workflow::expand_matrix(j).len(),
                })).collect::<Vec<_>>(),
            },
        }))
        .into_response()),
        Err(e) => Ok((
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "code": "CI-400",
                "message": e.message,
                "path": e.path,
            })),
        )
            .into_response()),
    }
}

// ---------- Runner 池(T-302) ----------

#[derive(Debug, Deserialize)]
struct RegisterRunnerRequest {
    /// runner id(缺省自动生成)。
    pub id: Option<String>,
    /// 标签(job runs-on 匹配)。
    pub labels: Vec<String>,
    /// 类型:shell / docker(image 可选,缺省 alpine:3)。
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Docker 镜像。
    pub image: Option<String>,
}

fn default_kind() -> String {
    "shell".to_owned()
}

/// 注册 runner(外部执行器复用入口;同 id 覆盖)。
async fn register_runner(
    axum::Extension(_user): axum::Extension<CurrentUser>,
    State(st): State<CiState>,
    Json(body): Json<RegisterRunnerRequest>,
) -> Result<Response, AppError> {
    let id = body
        .id
        .clone()
        .unwrap_or_else(|| format!("runner-{}", uuid::Uuid::new_v4().simple()));
    let kind = match body.kind.as_str() {
        "docker" => crate::runner::RunnerKind::Docker {
            image: body.image.clone().unwrap_or_else(|| "alpine:3".to_owned()),
        },
        _ => crate::runner::RunnerKind::Shell,
    };
    let runner = st.runners.register(id, body.labels, kind).await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({"code": "OK", "data": {
            "id": runner.id,
            "labels": runner.labels,
            "kind": body.kind,
        }})),
    )
        .into_response())
}

async fn list_runners(
    axum::Extension(_user): axum::Extension<CurrentUser>,
    State(st): State<CiState>,
) -> Result<Response, AppError> {
    let runners = st.runners.list().await;
    Ok(Json(serde_json::json!({"code": "OK", "data": runners})).into_response())
}

async fn runner_heartbeat(
    axum::Extension(_user): axum::Extension<CurrentUser>,
    State(st): State<CiState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    st.runners.heartbeat(&id).await?;
    Ok(Json(serde_json::json!({"code": "OK"})).into_response())
}

// ---------- Pipeline / Run(T-303) ----------

#[derive(Debug, Deserialize)]
struct SavePipelineRequest {
    /// 名称(仓库内唯一)。
    pub name: String,
    /// workflow YAML 全文。
    pub yaml: String,
}

fn repo_full_name(owner: &str, key: &str) -> String {
    format!("{owner}/{key}")
}

async fn repo_id_of(
    pool: &sqlx::PgPool,
    full_name: &str,
) -> Result<codeplan_kernel::ids::RepoId, AppError> {
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT id FROM repo_repository WHERE full_name = $1")
            .bind(full_name)
            .fetch_optional(pool)
            .await
            .map_err(|e| AppError::internal("CI").with_details(serde_json::json!(e.to_string())))?;
    row.map(|(id,)| codeplan_kernel::ids::RepoId::from_uuid(id))
        .ok_or_else(|| AppError::not_found("CI", "仓库不存在"))
}

fn pipeline_json(p: &crate::store::Pipeline) -> serde_json::Value {
    serde_json::json!({
        "id": p.id,
        "name": p.name,
        "yaml": p.yaml,
        "enabled": p.enabled,
        "created_at": p.created_at.to_rfc3339(),
    })
}

async fn save_pipeline(
    axum::Extension(user): axum::Extension<CurrentUser>,
    State(st): State<CiState>,
    axum::extract::Path((owner, key)): axum::extract::Path<(String, String)>,
    Json(body): Json<SavePipelineRequest>,
) -> Result<Response, AppError> {
    let _ = user;
    let Some(pool) = st.pool.clone() else {
        return Err(AppError::internal("CI").with_details(serde_json::json!("pool 未装配")));
    };
    let repo_id = repo_id_of(&pool, &repo_full_name(&owner, &key)).await?;
    let wf = crate::workflow::parse_workflow(&body.yaml)
        .map_err(|e| AppError::invalid("CI", format!("{e}")))?;
    let parsed = serde_json::to_value(&wf)
        .map_err(|e| AppError::internal("CI").with_details(serde_json::json!(e.to_string())))?;
    let id = st
        .store
        .save_pipeline(repo_id, &body.name, &body.yaml, &parsed)
        .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({"code": "OK", "data": {"id": id, "name": body.name, "jobs": wf.jobs.len(), "triggers": wf.triggers}})),
    )
        .into_response())
}

async fn list_pipelines(
    axum::Extension(_user): axum::Extension<CurrentUser>,
    State(st): State<CiState>,
    axum::extract::Path((owner, key)): axum::extract::Path<(String, String)>,
) -> Result<Response, AppError> {
    let Some(pool) = st.pool.clone() else {
        return Err(AppError::internal("CI").with_details(serde_json::json!("pool 未装配")));
    };
    let repo_id = repo_id_of(&pool, &repo_full_name(&owner, &key)).await?;
    let pipelines = st.store.list_pipelines(repo_id).await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": pipelines
        .iter()
        .map(pipeline_json)
        .collect::<Vec<_>>()}))
    .into_response())
}

async fn delete_pipeline(
    axum::Extension(_user): axum::Extension<CurrentUser>,
    State(st): State<CiState>,
    axum::extract::Path((owner, key, id)): axum::extract::Path<(String, String, uuid::Uuid)>,
) -> Result<Response, AppError> {
    let Some(pool) = st.pool.clone() else {
        return Err(AppError::internal("CI").with_details(serde_json::json!("pool 未装配")));
    };
    let repo_id = repo_id_of(&pool, &repo_full_name(&owner, &key)).await?;
    let p = st
        .store
        .pipeline_by_id(id)
        .await?
        .ok_or_else(|| AppError::not_found("CI", "pipeline 不存在"))?;
    if p.repo_id != repo_id {
        return Err(AppError::not_found("CI", "pipeline 不存在"));
    }
    st.store.delete_pipeline(id).await?;
    Ok(Json(serde_json::json!({"code": "OK"})).into_response())
}

#[derive(Debug, Deserialize)]
struct DispatchRequest {
    /// 分支(缺省 main)。
    #[serde(default = "default_branch")]
    pub branch: String,
    /// 提交 SHA(可空:checkout 分支头)。
    pub sha: Option<String>,
}

fn default_branch() -> String {
    "main".to_owned()
}

async fn dispatch_pipeline(
    axum::Extension(user): axum::Extension<CurrentUser>,
    State(st): State<CiState>,
    axum::extract::Path((owner, key, id)): axum::extract::Path<(String, String, uuid::Uuid)>,
    body: Option<Json<DispatchRequest>>,
) -> Result<Response, AppError> {
    let Some(pool) = st.pool.clone() else {
        return Err(AppError::internal("CI").with_details(serde_json::json!("pool 未装配")));
    };
    let repo_id = repo_id_of(&pool, &repo_full_name(&owner, &key)).await?;
    let p = st
        .store
        .pipeline_by_id(id)
        .await?
        .ok_or_else(|| AppError::not_found("CI", "pipeline 不存在"))?;
    if p.repo_id != repo_id {
        return Err(AppError::not_found("CI", "pipeline 不存在"));
    }
    let engine = st
        .engine
        .clone()
        .ok_or_else(|| AppError::internal("CI").with_details(serde_json::json!("引擎未启用")))?;
    let (branch, sha) = match &body {
        Some(Json(b)) => (b.branch.clone(), b.sha.clone().unwrap_or_default()),
        None => ("main".to_owned(), String::new()),
    };
    let run_id = st
        .store
        .create_run(
            id,
            repo_id,
            &branch,
            &sha,
            "manual",
            Some(user.id.as_uuid()),
        )
        .await?;
    engine.enqueue(run_id).await?;
    Ok((
        axum::http::StatusCode::ACCEPTED,
        Json(serde_json::json!({"code": "OK", "data": {"run_id": run_id, "status": "queued"}})),
    )
        .into_response())
}

async fn list_runs(
    axum::Extension(_user): axum::Extension<CurrentUser>,
    State(st): State<CiState>,
    axum::extract::Path((owner, key)): axum::extract::Path<(String, String)>,
) -> Result<Response, AppError> {
    let Some(pool) = st.pool.clone() else {
        return Err(AppError::internal("CI").with_details(serde_json::json!("pool 未装配")));
    };
    let repo_id = repo_id_of(&pool, &repo_full_name(&owner, &key)).await?;
    let runs = st.store.list_runs(repo_id, 30).await?;
    Ok(Json(serde_json::json!({"code": "OK", "data": runs})).into_response())
}

async fn run_detail(
    axum::Extension(_user): axum::Extension<CurrentUser>,
    State(st): State<CiState>,
    axum::extract::Path(id): axum::extract::Path<uuid::Uuid>,
) -> Result<Response, AppError> {
    let run = st
        .store
        .run_by_id(id)
        .await?
        .ok_or_else(|| AppError::not_found("CI", "run 不存在"))?;
    let steps = st.store.list_steps(id).await?;
    Ok(
        Json(serde_json::json!({"code": "OK", "data": {"run": run, "steps": steps}}))
            .into_response(),
    )
}

/// SSE 实时事件流(run 粒度:queued/step_started/step_finished/run_finished)。
async fn run_logs_sse(
    axum::Extension(_user): axum::Extension<CurrentUser>,
    State(st): State<CiState>,
    axum::extract::Path(id): axum::extract::Path<uuid::Uuid>,
) -> Result<Response, AppError> {
    let engine = st
        .engine
        .clone()
        .ok_or_else(|| AppError::internal("CI").with_details(serde_json::json!("引擎未启用")))?;
    let rx = engine.sse.subscribe();
    let stream = futures_util::stream::unfold(rx, move |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if event.run_id != id {
                        continue;
                    }
                    let data = serde_json::to_string(&event).unwrap_or_default();
                    let sse_event = axum::response::sse::Event::default()
                        .event(event.kind.clone())
                        .data(data);
                    return Some((Ok::<_, std::convert::Infallible>(sse_event), rx));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return None,
            }
        }
    });
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("text/event-stream"),
        )],
        axum::response::Sse::new(stream),
    )
        .into_response())
}
