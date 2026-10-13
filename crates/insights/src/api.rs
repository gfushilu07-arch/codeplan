//! 报表数据源(PG 只读投影)与 API(T-212)。

use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use codeplan_common::error::AppError;
use codeplan_kernel::authn::TokenAuthenticator;
use codeplan_kernel::identity::CurrentUser;
use serde::Deserialize;
use sqlx::PgPool;

use crate::export;
use crate::reports::{self, ItemRow};

/// API 状态。
#[derive(Clone)]
pub struct InsightsState {
    /// 只读连接池。
    pub pool: PgPool,
    /// 令牌认证器。
    pub authn: std::sync::Arc<dyn TokenAuthenticator>,
}

/// 路由。
pub fn routes(state: InsightsState) -> Router<InsightsState> {
    Router::new()
        .route(
            "/api/projects/{key}/reports/sprint/{sprint_id}",
            get(sprint_report),
        )
        .route("/api/projects/{key}/reports/load", get(assignee_load))
        .route("/api/projects/{key}/reports/bugs", get(bug_trend))
        .route("/api/projects/{key}/reports/throughput", get(throughput))
        .route("/api/projects/{key}/reports/cfd", get(cumulative_flow))
        .route(
            "/api/projects/{key}/reports/export/{kind}",
            get(export_report),
        )
        .route("/api/repos/{owner}/{key}/dora", get(dora_summary))
        .route("/api/repos/{owner}/{key}/dora/{metric}", get(dora_drilldown))
        .route(
            "/api/projects/{key}/reports/efficiency",
            get(project_efficiency),
        )
        .route(
            "/api/repos/{owner}/{key}/reports/pr-metrics",
            get(pr_metrics),
        )
        .route(
            "/api/repos/{owner}/{key}/external-links",
            get(repo_external_links),
        )
        .route(
            "/api/admin/external-links",
            get(list_external_links).post(upsert_external_link),
        )
        .route(
            "/api/admin/external-links/{id}",
            axum::routing::delete(delete_external_link),
        )
        .layer(middleware::from_fn_with_state(state, auth_mw))
}

async fn auth_mw(
    State(st): State<InsightsState>,
    mut req: axum::extract::Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::unauthenticated("IN"))?;
    let user = st
        .authn
        .verify(token)
        .await
        .map_err(|_| AppError::unauthenticated("IN"))?;
    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

/// query_as 元组(超出 clippy 复杂度阈值,提为别名)。
type ItemTuple = (
    String,
    String,
    Option<uuid::Uuid>,
    Option<i64>,
    Option<uuid::Uuid>,
    chrono::DateTime<Utc>,
    chrono::DateTime<Utc>,
);

async fn items_of_project(pool: &PgPool, key: &str) -> Result<Vec<ItemRow>, AppError> {
    let rows: Vec<ItemTuple> = sqlx::query_as(
        "SELECT w.item_type, w.status, w.assignee,
                    COALESCE((w.custom_fields->>'story_points')::bigint, NULL),
                    w.sprint_id, w.created_at, w.updated_at
             FROM proj_work_item w JOIN proj_project p ON p.id = w.project_id
             WHERE p.key = $1",
    )
    .bind(key)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::internal("IN").with_details(serde_json::json!(e.to_string())))?;
    Ok(rows
        .into_iter()
        .map(
            |(item_type, status, assignee, story_points, sprint_id, created_at, updated_at)| {
                ItemRow {
                    item_type,
                    status,
                    assignee,
                    story_points,
                    sprint_id,
                    created_at,
                    updated_at,
                }
            },
        )
        .collect())
}

#[derive(Debug, Deserialize)]
struct DaysQuery {
    /// 天数(缺省 14)。
    pub days: Option<u32>,
}

async fn sprint_report(
    State(st): State<InsightsState>,
    Path((key, sprint_id)): Path<(String, uuid::Uuid)>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let _ = req;
    let items = items_of_project(&st.pool, &key).await?;
    let r = reports::sprint_report(&items, sprint_id);
    Ok(Json(serde_json::json!({"code": "OK", "data": r})).into_response())
}

async fn assignee_load(
    State(st): State<InsightsState>,
    Path(key): Path<String>,
    req: axum::Extension<CurrentUser>,
) -> Result<Response, AppError> {
    let _ = req;
    let items = items_of_project(&st.pool, &key).await?;
    let r = reports::assignee_load(&items);
    Ok(Json(serde_json::json!({"code": "OK", "data": r})).into_response())
}

async fn bug_trend(
    State(st): State<InsightsState>,
    Path(key): Path<String>,
    req: axum::Extension<CurrentUser>,
    Query(q): Query<DaysQuery>,
) -> Result<Response, AppError> {
    let _ = req;
    let items = items_of_project(&st.pool, &key).await?;
    let r = reports::bug_trend(&items, q.days.unwrap_or(14));
    Ok(Json(serde_json::json!({"code": "OK", "data": r})).into_response())
}

async fn throughput(
    State(st): State<InsightsState>,
    Path(key): Path<String>,
    req: axum::Extension<CurrentUser>,
    Query(q): Query<DaysQuery>,
) -> Result<Response, AppError> {
    let _ = req;
    let items = items_of_project(&st.pool, &key).await?;
    let r = reports::throughput(&items, q.days.unwrap_or(8));
    Ok(Json(serde_json::json!({"code": "OK", "data": r})).into_response())
}

async fn cumulative_flow(
    State(st): State<InsightsState>,
    Path(key): Path<String>,
    req: axum::Extension<CurrentUser>,
    Query(q): Query<DaysQuery>,
) -> Result<Response, AppError> {
    let _ = req;
    let items = items_of_project(&st.pool, &key).await?;
    let r = reports::cumulative_flow(&items, q.days.unwrap_or(14));
    Ok(Json(serde_json::json!({"code": "OK", "data": r})).into_response())
}

#[derive(Debug, Deserialize)]
struct ExportQuery {
    /// 报表种类:sprint / load / bugs / throughput / cfd。
    pub kind: String,
    /// 迭代 ID(sprint 报表必填)。
    pub sprint_id: Option<uuid::Uuid>,
    /// 天数/周数。
    pub days: Option<u32>,
    /// 格式:xlsx / pdf(缺省 xlsx)。
    pub format: Option<String>,
}

async fn export_report(
    State(st): State<InsightsState>,
    Path((key, _)): Path<(String, String)>,
    req: axum::Extension<CurrentUser>,
    Query(q): Query<ExportQuery>,
) -> Result<Response, AppError> {
    let _ = req;
    let items = items_of_project(&st.pool, &key).await?;
    let days = q.days.unwrap_or(14);
    let (title, lines, rows3, headers) = match q.kind.as_str() {
        "sprint" => {
            let sprint_id = q
                .sprint_id
                .ok_or_else(|| AppError::invalid("IN", "sprint 报表需要 sprint_id"))?;
            let r = reports::sprint_report(&items, sprint_id);
            (
                "Sprint Report".to_owned(),
                export::sprint_report_lines(&r),
                vec![("report".to_owned(), r.done, r.open)],
                ["metric", "done", "open"],
            )
        }
        "load" => {
            let l = reports::assignee_load(&items);
            (
                "Assignee Load".to_owned(),
                export::load_lines(&l),
                l.iter()
                    .map(|x| (x.assignee.to_string(), x.open_items, x.open_points))
                    .collect(),
                ["assignee", "open_items", "open_points"],
            )
        }
        "bugs" => {
            let t = reports::bug_trend(&items, days);
            (
                "Bug Trend".to_owned(),
                export::trend_lines(&t),
                t.iter()
                    .map(|x| (x.date.clone(), x.created, x.closed))
                    .collect(),
                ["date", "created", "closed"],
            )
        }
        "throughput" => {
            let t = reports::throughput(&items, days);
            (
                "Throughput".to_owned(),
                export::throughput_lines(&t),
                t.iter().map(|x| (x.week.clone(), x.done, 0)).collect(),
                ["week", "done", "-"],
            )
        }
        "cfd" => {
            let t = reports::cumulative_flow(&items, days);
            (
                "Cumulative Flow".to_owned(),
                export::cfd_lines(&t),
                t.iter()
                    .map(|x| (x.date.clone(), x.created, x.wip))
                    .collect(),
                ["date", "created", "wip"],
            )
        }
        _ => return Err(AppError::invalid("IN", "kind 非法")),
    };

    match q.format.as_deref().unwrap_or("xlsx") {
        "pdf" => {
            let bytes = export::text_pdf(&title, &lines)
                .map_err(|e| AppError::internal("IN").with_details(serde_json::json!(e)))?;
            Ok((
                StatusCode::OK,
                [(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"report-{}.pdf\"", q.kind),
                )],
                bytes,
            )
                .into_response())
        }
        _ => {
            let bytes = export::series_xlsx(headers, &rows3)
                .map_err(|e| AppError::internal("IN").with_details(serde_json::json!(e)))?;
            Ok((
                StatusCode::OK,
                [(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"report-{}.xlsx\"", q.kind),
                )],
                bytes,
            )
                .into_response())
        }
    }
}

// ---------- DORA(T-307) ----------

/// 窗口参数。
#[derive(Debug, Deserialize)]
pub struct DoraQuery {
    /// 统计天数(缺省 30,上限 365)。
    pub days: Option<i64>,
}

/// 仓库定位(只读投影)。
async fn repo_id_of(pool: &PgPool, owner: &str, key: &str) -> Result<uuid::Uuid, AppError> {
    let full_name = format!("{owner}/{key}");
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT id FROM repo_repository WHERE full_name = $1")
            .bind(&full_name)
            .fetch_optional(pool)
            .await
            .map_err(|e| AppError::internal("MS").with_details(serde_json::json!(e.to_string())))?;
    row.map(|(id,)| id)
        .ok_or_else(|| AppError::not_found("MS", "仓库不存在"))
}

/// 窗口起点。
fn since(days: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now() - chrono::Duration::days(days.clamp(1, 365))
}

/// DORA 四指标汇总。
#[utoipa::path(
    get,
    path = "/api/repos/{owner}/{key}/dora",
    params(("owner" = String, Path), ("key" = String, Path)),
    responses((status = 200))
)]
async fn dora_summary(
    axum::Extension(_user): axum::Extension<CurrentUser>,
    State(st): State<InsightsState>,
    Path((owner, key)): Path<(String, String)>,
    Query(q): Query<DoraQuery>,
) -> Result<Json<codeplan_common::response::ApiResponse<crate::dora::DoraSummary>>, AppError> {
    let days = q.days.unwrap_or(30).clamp(1, 365);
    let repo_id = repo_id_of(&st.pool, &owner, &key).await?;
    let store = crate::dora::PgDoraStore::new(st.pool.clone());
    let facts = store.facts_since(repo_id, since(days)).await?;
    Ok(Json(codeplan_common::response::ApiResponse::ok(
        crate::dora::summarize(&facts, days),
    )))
}

/// 指标下钻:返回该指标口径下的事实明细。
#[utoipa::path(
    get,
    path = "/api/repos/{owner}/{key}/dora/{metric}",
    params(("owner" = String, Path), ("key" = String, Path), ("metric" = String, Path)),
    responses((status = 200), (status = 400))
)]
async fn dora_drilldown(
    axum::Extension(_user): axum::Extension<CurrentUser>,
    State(st): State<InsightsState>,
    Path((owner, key, metric)): Path<(String, String, String)>,
    Query(q): Query<DoraQuery>,
) -> Result<Json<codeplan_common::response::ApiResponse<Vec<crate::dora::Fact>>>, AppError> {
    use crate::dora::FactKind;
    let days = q.days.unwrap_or(30).clamp(1, 365);
    // 指标 → 事实口径
    let wanted: &[FactKind] = match metric.as_str() {
        "deployment_frequency" => &[FactKind::Deploy],
        "lead_time" => &[FactKind::Change],
        "change_failure_rate" | "mttr" => &[FactKind::Deploy, FactKind::Failure],
        other => {
            return Err(AppError::invalid(
                "MS",
                format!("未知指标:{other}(可选 deployment_frequency/lead_time/change_failure_rate/mttr)"),
            ));
        }
    };
    let repo_id = repo_id_of(&st.pool, &owner, &key).await?;
    let store = crate::dora::PgDoraStore::new(st.pool.clone());
    let facts: Vec<crate::dora::Fact> = store
        .facts_since(repo_id, since(days))
        .await?
        .into_iter()
        .filter(|f| wanted.contains(&f.kind))
        .collect();
    Ok(Json(codeplan_common::response::ApiResponse::ok(facts)))
}

// ---------- 效能报表 / 外部链接器(T-308) ----------

/// 项目级效能指标(交付周期 + 缺陷逃逸)。
#[derive(Debug, serde::Serialize)]
pub struct ProjectEfficiency {
    /// 交付周期(终态工作项)。
    pub delivery_cycle: crate::stats::DurationStats,
    /// 缺陷逃逸。
    pub defect_escape: crate::efficiency::DefectEscape,
}

#[utoipa::path(
    get,
    path = "/api/projects/{key}/reports/efficiency",
    params(("key" = String, Path)),
    responses((status = 200))
)]
async fn project_efficiency(
    axum::Extension(_user): axum::Extension<CurrentUser>,
    State(st): State<InsightsState>,
    Path(key): Path<String>,
) -> Result<Json<codeplan_common::response::ApiResponse<ProjectEfficiency>>, AppError> {
    let items = items_of_project(&st.pool, &key).await?;
    // 迭代结束时间(判定缺陷是否逃逸)
    let rows: Vec<(uuid::Uuid, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        "SELECT s.id, s.ends_at FROM proj_sprint s JOIN proj_project p ON p.id = s.project_id \
         WHERE p.key = $1 AND s.ends_at IS NOT NULL",
    )
    .bind(&key)
    .fetch_all(&st.pool)
    .await
    .map_err(|e| AppError::internal("MS").with_details(serde_json::json!(e.to_string())))?;
    let ends: std::collections::HashMap<uuid::Uuid, chrono::DateTime<chrono::Utc>> =
        rows.into_iter().filter_map(|(id, e)| e.map(|e| (id, e))).collect();
    Ok(Json(codeplan_common::response::ApiResponse::ok(
        ProjectEfficiency {
            delivery_cycle: crate::efficiency::delivery_cycle(&items),
            defect_escape: crate::efficiency::defect_escape(&items, &ends),
        },
    )))
}

/// 仓库级 PR 效能(PR 周期 + 评审响应)。
#[derive(Debug, serde::Serialize)]
pub struct PrMetrics {
    /// PR 周期。
    pub pr_cycle: crate::stats::DurationStats,
    /// 评审响应。
    pub review_response: crate::stats::DurationStats,
}

#[utoipa::path(
    get,
    path = "/api/repos/{owner}/{key}/reports/pr-metrics",
    params(("owner" = String, Path), ("key" = String, Path)),
    responses((status = 200), (status = 404))
)]
async fn pr_metrics(
    axum::Extension(_user): axum::Extension<CurrentUser>,
    State(st): State<InsightsState>,
    Path((owner, key)): Path<(String, String)>,
) -> Result<Json<codeplan_common::response::ApiResponse<PrMetrics>>, AppError> {
    let repo_id = repo_id_of(&st.pool, &owner, &key).await?;
    let prs: Vec<(uuid::Uuid, chrono::DateTime<chrono::Utc>, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as(
            "SELECT id, created_at, merged_at FROM rev_pull_request WHERE repo_id = $1",
        )
        .bind(repo_id)
        .fetch_all(&st.pool)
        .await
        .map_err(|e| AppError::internal("MS").with_details(serde_json::json!(e.to_string())))?;
    let reviews: Vec<(uuid::Uuid, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT r.pr_id, r.created_at FROM rev_pr_review r \
         JOIN rev_pull_request p ON p.id = r.pr_id WHERE p.repo_id = $1",
    )
    .bind(repo_id)
    .fetch_all(&st.pool)
    .await
    .map_err(|e| AppError::internal("MS").with_details(serde_json::json!(e.to_string())))?;

    let pr_rows: Vec<crate::efficiency::PrRow> = prs
        .into_iter()
        .map(|(id, created_at, merged_at)| crate::efficiency::PrRow {
            id,
            created_at,
            merged_at,
        })
        .collect();
    let review_rows: Vec<crate::efficiency::ReviewRow> = reviews
        .into_iter()
        .map(|(pr_id, created_at)| crate::efficiency::ReviewRow { pr_id, created_at })
        .collect();
    Ok(Json(codeplan_common::response::ApiResponse::ok(PrMetrics {
        pr_cycle: crate::efficiency::pr_cycle_time(&pr_rows),
        review_response: crate::efficiency::review_response_time(&pr_rows, &review_rows),
    })))
}

/// 解析后的外部链接。
#[derive(Debug, serde::Serialize)]
pub struct ResolvedLinkResponse {
    /// 展示名。
    pub name: String,
    /// 最终 URL。
    pub url: String,
}

/// 仓库页外部链接(按 sha/分支上下文解析模板)。
#[utoipa::path(
    get,
    path = "/api/repos/{owner}/{key}/external-links",
    params(("owner" = String, Path), ("key" = String, Path)),
    responses((status = 200))
)]
async fn repo_external_links(
    axum::Extension(_user): axum::Extension<CurrentUser>,
    State(st): State<InsightsState>,
    Path((owner, key)): Path<(String, String)>,
    Query(q): Query<LinkContextQuery>,
) -> Result<Json<codeplan_common::response::ApiResponse<Vec<ResolvedLinkResponse>>>, AppError> {
    let repo_id = repo_id_of(&st.pool, &owner, &key).await?;
    let store = crate::linker::PgLinkStore::new(st.pool.clone());
    let links = store
        .list_for(crate::linker::LinkScope::Repo, Some(repo_id))
        .await?;
    let mut ctx = std::collections::HashMap::new();
    ctx.insert("repo".to_owned(), format!("{owner}/{key}"));
    if let Some(sha) = q.sha.filter(|s| !s.trim().is_empty()) {
        ctx.insert("sha".to_owned(), sha);
    }
    if let Some(branch) = q.branch.filter(|s| !s.trim().is_empty()) {
        ctx.insert("branch".to_owned(), branch);
    }
    Ok(Json(codeplan_common::response::ApiResponse::ok(
        links
            .into_iter()
            .map(|l| ResolvedLinkResponse {
                name: l.name,
                url: crate::linker::render(&l.url_template, &ctx),
            })
            .collect(),
    )))
}

/// 链接解析上下文参数。
#[derive(Debug, Deserialize)]
pub struct LinkContextQuery {
    /// 提交 SHA。
    pub sha: Option<String>,
    /// 分支。
    pub branch: Option<String>,
}

/// 全平台外部链接条目。
#[derive(Debug, serde::Serialize)]
pub struct LinkResponse {
    /// ID。
    pub id: String,
    /// 展示名。
    pub name: String,
    /// URL 模板。
    pub url_template: String,
}

/// 新增/更新外部链接请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpsertLinkRequest {
    /// 展示名。
    pub name: String,
    /// URL 模板(占位符:repo/sha/branch/item_number/pr_number/tag)。
    pub url_template: String,
}

fn require_admin(user: CurrentUser) -> Result<(), AppError> {
    if !user.is_platform_admin {
        return Err(AppError::forbidden("MS", "仅平台管理员可维护外部链接"));
    }
    Ok(())
}

/// 列出全平台外部链接。
#[utoipa::path(get, path = "/api/admin/external-links", responses((status = 200)))]
async fn list_external_links(
    axum::Extension(user): axum::Extension<CurrentUser>,
    State(st): State<InsightsState>,
) -> Result<Json<codeplan_common::response::ApiResponse<Vec<LinkResponse>>>, AppError> {
    require_admin(user)?;
    let store = crate::linker::PgLinkStore::new(st.pool.clone());
    let links = store
        .list_for(crate::linker::LinkScope::Global, None)
        .await?;
    Ok(Json(codeplan_common::response::ApiResponse::ok(
        links
            .into_iter()
            .map(|l| LinkResponse {
                id: l.id.to_string(),
                name: l.name,
                url_template: l.url_template,
            })
            .collect(),
    )))
}

/// 新增/更新外部链接(模板含未知占位符 → 400)。
#[utoipa::path(post, path = "/api/admin/external-links", responses((status = 200), (status = 400)))]
async fn upsert_external_link(
    axum::Extension(user): axum::Extension<CurrentUser>,
    State(st): State<InsightsState>,
    Json(req): Json<UpsertLinkRequest>,
) -> Result<Json<codeplan_common::response::ApiResponse<LinkResponse>>, AppError> {
    require_admin(user)?;
    let name = req.name.trim();
    if name.is_empty() {
        return Err(AppError::invalid("MS", "名称不能为空"));
    }
    let unknown = crate::linker::unknown_placeholders(&req.url_template);
    if !unknown.is_empty() {
        return Err(AppError::invalid(
            "MS",
            format!(
                "模板含未知占位符:{};可用:{}",
                unknown.join(", "),
                crate::linker::PLACEHOLDERS.join(", ")
            ),
        ));
    }
    let store = crate::linker::PgLinkStore::new(st.pool.clone());
    let id = store
        .upsert(
            crate::linker::LinkScope::Global,
            None,
            name,
            req.url_template.trim(),
        )
        .await?;
    Ok(Json(codeplan_common::response::ApiResponse::ok(
        LinkResponse {
            id: id.to_string(),
            name: name.to_owned(),
            url_template: req.url_template.trim().to_owned(),
        },
    )))
}

/// 删除外部链接。
#[utoipa::path(delete, path = "/api/admin/external-links/{id}", responses((status = 200), (status = 404)))]
async fn delete_external_link(
    axum::Extension(user): axum::Extension<CurrentUser>,
    State(st): State<InsightsState>,
    Path(id): Path<String>,
) -> Result<Json<codeplan_common::response::ApiResponse<()>>, AppError> {
    require_admin(user)?;
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| AppError::invalid("MS", "链接 ID 非法"))?;
    let store = crate::linker::PgLinkStore::new(st.pool.clone());
    if !store.delete(uuid).await? {
        return Err(AppError::not_found("MS", "链接不存在"));
    }
    Ok(Json(codeplan_common::response::ApiResponse {
        code: "OK".to_owned(),
        message: "已删除".to_owned(),
        data: None,
    }))
}
