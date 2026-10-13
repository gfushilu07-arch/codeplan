//! 安全扫描端点(T-305):触发扫描 + 查询发现。
//!
//! 门禁不在本域判定:扫描结束发布 `ci.scan.finished`(含严重程度计数),
//! 由组合根桥接为 PR 提交状态(与 `ci.run.finished` 同构,L3 之间不互相依赖)。

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::post;
use axum::{Json, Router};
use codeplan_common::error::AppError;
use codeplan_common::response::ApiResponse;
use codeplan_kernel::authn::TokenAuthenticator;
use codeplan_kernel::events::{EventBus, EventEnvelope};
use codeplan_kernel::identity::CurrentUser;
use serde::Deserialize;
use serde_json::json;
use utoipa::{IntoParams, ToSchema};

use crate::DOMAIN_CODE;
use crate::scan::{PgScanStore, ScannerTool, StoredFinding};

/// 扫描 API 状态。
#[derive(Clone)]
pub struct ScanState {
    /// 令牌认证器。
    pub authn: Arc<dyn TokenAuthenticator>,
    /// 发现存储。
    pub store: Arc<PgScanStore>,
    /// PG(仓库定位)。
    pub pool: sqlx::PgPool,
    /// 裸仓库根(GIT_PROJECT_ROOT)。
    pub repo_root: String,
    /// 事件总线。
    pub events: Arc<dyn EventBus>,
}

/// 路由。
pub fn routes(state: ScanState) -> Router<ScanState> {
    Router::new()
        .route(
            "/api/repos/{owner}/{key}/scans",
            post(run_scan).get(list_findings),
        )
        .layer(middleware::from_fn_with_state(state, auth_mw))
}

/// 认证中间件(Bearer → `CurrentUser`)。
async fn auth_mw(
    State(st): State<ScanState>,
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

/// 触发扫描请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct RunScanRequest {
    /// 工具:gitleaks / semgrep / trivy。
    pub tool: String,
    /// 引用(分支/标签/SHA);缺省 `HEAD`。
    #[serde(default)]
    pub reference: Option<String>,
}

/// 扫描结果摘要。
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct ScanSummary {
    /// 工具。
    pub tool: String,
    /// 解析后的提交 SHA。
    pub commit_sha: String,
    /// 发现总数。
    pub total: usize,
    /// 严重程度 → 计数。
    pub counts: std::collections::BTreeMap<String, i64>,
    /// 是否触发门禁失败(critical/high 存在)。
    pub gated: bool,
}

/// 查询参数。
#[derive(Debug, Deserialize, IntoParams)]
pub struct FindingsQuery {
    /// 提交 SHA(缺省取默认分支 HEAD)。
    pub commit_sha: Option<String>,
}

/// 发现响应。
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct FindingResponse {
    /// 工具。
    pub tool: String,
    /// 严重程度。
    pub severity: String,
    /// 规则 ID。
    pub rule_id: String,
    /// 文件路径。
    pub file_path: String,
    /// 行号。
    pub line: Option<i64>,
    /// 描述。
    pub message: String,
}

fn to_response(f: &StoredFinding) -> FindingResponse {
    FindingResponse {
        tool: f.tool.clone(),
        severity: f.severity.clone(),
        rule_id: f.rule_id.clone(),
        file_path: f.file_path.clone(),
        line: f.line,
        message: f.message.clone(),
    }
}

/// 仓库定位:`full_name → (repo_id, 裸仓库路径)`。
async fn locate(
    pool: &sqlx::PgPool,
    repo_root: &str,
    owner: &str,
    key: &str,
) -> Result<(uuid::Uuid, std::path::PathBuf), AppError> {
    let full_name = format!("{owner}/{key}");
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT id FROM repo_repository WHERE full_name = $1")
            .bind(&full_name)
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
            })?;
    let repo_id = row
        .map(|(id,)| id)
        .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "仓库不存在"))?;
    Ok((
        repo_id,
        std::path::PathBuf::from(format!("{repo_root}/{full_name}.git")),
    ))
}

/// 解析引用 → 提交 SHA。
async fn resolve_sha(bare: &std::path::Path, reference: &str) -> Result<String, AppError> {
    let out = tokio::process::Command::new("git")
        .arg(format!("--git-dir={}", bare.display()))
        .args(["rev-parse", reference])
        .output()
        .await
        .map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
        })?;
    if !out.status.success() {
        return Err(AppError::invalid(
            DOMAIN_CODE,
            format!(
                "引用解析失败({reference}):{}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// 触发一次扫描:容器执行 → 落库 → 发布 `ci.scan.finished`。
#[utoipa::path(
    post,
    path = "/api/repos/{owner}/{key}/scans",
    request_body = RunScanRequest,
    params(("owner" = String, Path), ("key" = String, Path)),
    responses((status = 200, body = ScanSummary), (status = 404))
)]
async fn run_scan(
    axum::Extension(_user): axum::Extension<CurrentUser>,
    State(st): State<ScanState>,
    Path((owner, key)): Path<(String, String)>,
    Json(req): Json<RunScanRequest>,
) -> Result<Json<ApiResponse<ScanSummary>>, AppError> {
    let tool = ScannerTool::from_str_opt(&req.tool)
        .ok_or_else(|| AppError::invalid(DOMAIN_CODE, format!("不支持的工具:{}", req.tool)))?;
    let (repo_id, bare) = locate(&st.pool, &st.repo_root, &owner, &key).await?;
    let reference = req.reference.unwrap_or_else(|| "HEAD".to_owned());
    let commit_sha = resolve_sha(&bare, &reference).await?;

    let findings = crate::scan::run_scan(tool, &bare, &reference).await?;
    st.store
        .replace(repo_id, &commit_sha, tool, &findings)
        .await?;
    let counts = st
        .store
        .counts_by_severity(repo_id, &commit_sha)
        .await?;
    let gated = counts
        .get("critical")
        .copied()
        .unwrap_or(0)
        .max(counts.get("high").copied().unwrap_or(0))
        > 0;

    let full_name = format!("{owner}/{key}");
    let _ = st
        .events
        .publish(vec![EventEnvelope::new(
            "ci.scan.finished",
            json!({
                "repo_id": repo_id,
                "full_name": full_name,
                "commit_sha": commit_sha,
                "tool": tool.as_str(),
                "total": findings.len(),
                "counts": counts,
                "gated": gated,
            }),
        )])
        .await;

    Ok(Json(ApiResponse::ok(ScanSummary {
        tool: tool.as_str().to_owned(),
        commit_sha,
        total: findings.len(),
        counts,
        gated,
    })))
}

/// 查询某提交的扫描发现。
#[utoipa::path(
    get,
    path = "/api/repos/{owner}/{key}/scans",
    params(FindingsQuery, ("owner" = String, Path), ("key" = String, Path)),
    responses((status = 200, body = FindingResponse))
)]
async fn list_findings(
    axum::Extension(_user): axum::Extension<CurrentUser>,
    State(st): State<ScanState>,
    Path((owner, key)): Path<(String, String)>,
    Query(q): Query<FindingsQuery>,
) -> Result<Json<ApiResponse<Vec<FindingResponse>>>, AppError> {
    let (repo_id, bare) = locate(&st.pool, &st.repo_root, &owner, &key).await?;
    let sha = match q.commit_sha {
        Some(s) if !s.trim().is_empty() => s,
        _ => resolve_sha(&bare, "HEAD").await?,
    };
    let list = st.store.list(repo_id, &sha).await?;
    Ok(Json(ApiResponse::ok(
        list.iter().map(to_response).collect(),
    )))
}
