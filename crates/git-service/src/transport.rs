//! Git smart HTTP 传输(T-121):请求 → Basic 鉴权 → `git http-backend` CGI 代理。
//!
//! - 认证:HTTP Basic,密码位 = 登录会话 JWT(T-210 换 PAT);
//! - 授权:fetch 走仓库 Read,push(receive-pack)走 Write;
//! - 事件:push 后由 post-receive 钩子回调 `/internal/hooks/post-receive`。

use std::process::Stdio;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, post};
use base64::Engine as _;
use codeplan_common::error::AppError;
use codeplan_kernel::authz::Action;
use codeplan_kernel::identity::CurrentUser;
use serde::Deserialize;
use serde_json::json;
use tokio::process::Command;

use crate::DOMAIN_CODE;
use crate::service::GitService;

/// GitState 需要的额外字段(独立 struct,server 负责构造完整状态)。
#[derive(Clone)]
pub struct TransportState {
    /// Git 域服务(鉴权 + 元数据 + 事件)。
    pub svc: Arc<GitService>,
    /// 令牌认证器。
    pub authn: Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
    /// 仓库根(GIT_PROJECT_ROOT)。
    pub repo_root: String,
    /// 内部钩子密钥。
    pub hook_secret: String,
    /// LFS 子系统(None = 未启用)。
    pub lfs: Option<crate::lfs::LfsState>,
    /// 推送门禁(T-205;None = 不启用保护规则)。
    pub push_guard: Option<Arc<dyn codeplan_kernel::git::PushGuard>>,
}

/// Git 传输路由:`/git/{*rest}` 全方法。
pub fn routes(state: TransportState) -> axum::Router<TransportState> {
    Router::new()
        .route("/git/{*rest}", any(git_proxy))
        .route("/internal/hooks/post-receive", post(post_receive))
        .route("/internal/hooks/pre-receive", post(pre_receive))
        .with_state(state)
}

/// Basic 凭据解析。
struct BasicCredentials {
    #[allow(dead_code)]
    username: String,
    token: String,
}

fn basic_credentials(headers: &HeaderMap) -> Option<BasicCredentials> {
    let value = headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Basic ")?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(value)
        .ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (username, token) = text.split_once(':')?;
    Some(BasicCredentials {
        username: username.to_owned(),
        token: token.to_owned(),
    })
}

/// 从 git 路径提取仓库全名:`admin/demo.git/info/refs` → `admin/demo`。
fn full_name_from_path(rest: &str) -> Option<String> {
    let mut segs = rest.split('/').filter(|s| !s.is_empty());
    let owner = segs.next()?;
    let key_git = segs.next()?;
    let key = key_git.strip_suffix(".git")?;
    if owner.is_empty() || key.is_empty() {
        return None;
    }
    Some(format!("{owner}/{key}"))
}

fn is_push(req_method: &str, path: &str, query: Option<&str>) -> bool {
    if req_method == "POST" && path.contains("git-receive-pack") {
        return true;
    }
    query.is_some_and(|q| q.contains("service=git-receive-pack"))
}

/// Git 代理入口:鉴权 → CGI → 响应。
async fn git_proxy(State(st): State<TransportState>, req: Request) -> Result<Response, AppError> {
    let (parts, body) = req.into_parts();
    let method = parts.method.to_string();
    let query = parts.uri.query().map(str::to_owned);
    let rest = parts
        .uri
        .path()
        .strip_prefix("/git/")
        .unwrap_or_default()
        .to_owned();

    let Some(full_name) = full_name_from_path(&rest) else {
        return Err(AppError::not_found(DOMAIN_CODE, "仓库路径非法"));
    };
    let repo = st.svc.repository_by_full_name(&full_name).await?;

    // LFS 分流(T-202):info/lfs/* 独立认证,不走 CGI
    if crate::lfs::parse_route(&rest).is_some() {
        let Some(lfs) = st.lfs.clone() else {
            return Err(AppError::not_found(DOMAIN_CODE, "LFS 未启用"));
        };
        let req = Request::from_parts(parts, body);
        return crate::lfs::handle(st.clone(), lfs, repo, req).await;
    }

    // 鉴权(Basic;密码位 = 会话 JWT)。401 必须带 WWW-Authenticate 挑战头,
    // 否则 git 客户端不会自动携带凭据。
    let unauthorized = || -> Response {
        (
            StatusCode::UNAUTHORIZED,
            [(
                header::WWW_AUTHENTICATE,
                header::HeaderValue::from_static("Basic realm=\"codeplan\""),
            )],
        )
            .into_response()
    };
    let Some(creds) = basic_credentials(&parts.headers) else {
        return Ok(unauthorized());
    };
    let Ok(user) = st.authn.verify(&creds.token).await else {
        return Ok(unauthorized());
    };

    // 授权:push → Write,fetch → Read;PAT 需对应 scope(T-210)
    let push = is_push(&method, &rest, query.as_deref());
    let required_scope = if push { "repo:write" } else { "repo:read" };
    if !user.has_scope(required_scope) {
        return Ok((
            StatusCode::FORBIDDEN,
            format!("PAT 缺少 {required_scope} scope"),
        )
            .into_response());
    }
    let action = if push { Action::Write } else { Action::Read };
    let reference = codeplan_kernel::authz::ResourceRef::Repo(
        codeplan_kernel::ids::RepoId::from_uuid(repo.id.as_uuid()),
    );
    st.svc.ensure_action(&user, action, &reference).await?;

    // v1:整包读入内存(传输代理对中小仓库可接受;大对象流式随 T-304 优化)
    let body = axum::body::to_bytes(body, 512 * 1024 * 1024)
        .await
        .map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
        })?;

    let (status, headers, payload) = run_cgi(
        &st.repo_root,
        &rest,
        &method,
        query.as_deref(),
        &parts.headers,
        &body,
        &user,
    )
    .await?;

    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = (status, payload).into_response();
    *response.headers_mut() = headers;
    Ok(response)
}

/// 执行 `git http-backend` CGI 并解析响应。
async fn run_cgi(
    root: &str,
    rest: &str,
    method: &str,
    query: Option<&str>,
    req_headers: &HeaderMap,
    body: &[u8],
    user: &CurrentUser,
) -> Result<(u16, HeaderMap, Vec<u8>), AppError> {
    let mut cmd = Command::new("git");
    cmd.arg("http-backend")
        .env("GIT_PROJECT_ROOT", root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("PATH_INFO", format!("/{rest}"))
        .env("REQUEST_METHOD", method)
        .env("QUERY_STRING", query.unwrap_or_default())
        .env("REMOTE_USER", &user.display_name)
        .env("GIT_COMMITTER_NAME", &user.display_name)
        .env("GIT_COMMITTER_EMAIL", &user.email)
        .env("SERVER_PROTOCOL", "HTTP/1.1")
        .env("GATEWAY_INTERFACE", "CGI/1.1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let content_type = req_headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());
    if let Some(ct) = content_type {
        cmd.env("CONTENT_TYPE", ct);
    }
    cmd.env("CONTENT_LENGTH", body.len().to_string());

    let mut child = cmd.spawn().map_err(|e| {
        AppError::internal(DOMAIN_CODE)
            .with_details(serde_json::json!(format!("git http-backend 启动失败: {e}")))
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt as _;
        stdin.write_all(body).await.map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
        })?;
        stdin.shutdown().await.ok();
    }
    let out = child.wait_with_output().await.map_err(|e| {
        AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
    })?;

    // CGI 响应:头块 + \r\n\r\n + 正文;头块内可能含 "Status: 404 Not Found"
    let split_at = out
        .stdout
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| {
            AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(
                String::from_utf8_lossy(&out.stdout[..out.stdout.len().min(200)])
            ))
        })?;
    let header_block = String::from_utf8_lossy(&out.stdout[..split_at]).to_string();
    let payload = out.stdout[split_at + 4..].to_vec();

    let mut status = 200u16;
    let mut headers = HeaderMap::new();
    for line in header_block.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let key = k.trim();
        let value = v.trim();
        if key.eq_ignore_ascii_case("Status") {
            status = value
                .split_whitespace()
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(200);
            continue;
        }
        if key.eq_ignore_ascii_case("Status") {
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            header::HeaderName::try_from(key),
            header::HeaderValue::from_str(value),
        ) {
            headers.insert(name, value);
        }
    }
    Ok((status, headers, payload))
}

/// pre-receive 回调:逐行 `old new ref` → PushGuard 校验,拒绝时 403 + 原因。
async fn pre_receive(
    State(st): State<TransportState>,
    headers: HeaderMap,
    body: String,
) -> Result<Response, AppError> {
    let secret = headers
        .get("X-Hook-Secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if secret != st.hook_secret {
        return Err(AppError::forbidden(DOMAIN_CODE, "钩子密钥无效"));
    }
    let Some(guard) = st.push_guard.as_ref() else {
        return Ok((StatusCode::OK, axum::Json(json!({"code": "OK"}))).into_response());
    };
    let git_dir = headers
        .get("X-Hook-Repo")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let full_name = full_name_from_git_dir(git_dir, &st.repo_root);
    if full_name.is_empty() {
        // 仓库路径解析失败:无法定位规则,放行(与"无规则"同义),由服务日志暴露
        eprintln!("[pre-receive] 无法解析仓库路径: {git_dir}");
        return Ok((StatusCode::OK, axum::Json(json!({"code": "OK"}))).into_response());
    }
    let mut refs = Vec::new();
    for line in body.lines().filter(|l| !l.trim().is_empty()) {
        let mut it = line.split_whitespace();
        let (Some(old), Some(new), Some(ref_name)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        refs.push(codeplan_kernel::git::PushRef {
            ref_name: ref_name.to_owned(),
            old: old.to_owned(),
            new: new.to_owned(),
        });
    }
    if refs.is_empty() {
        return Ok((StatusCode::OK, "").into_response());
    }
    match guard.check_push(&full_name, &refs).await {
        Ok(force_checks) => Ok((StatusCode::OK, force_checks.join("\n")).into_response()),
        Err(reason) => Ok((StatusCode::FORBIDDEN, reason).into_response()),
    }
}

/// post-receive 回调体(原始 stdin 文本)。
#[derive(Debug, Deserialize)]
pub struct HookCallback {
    /// 钩子密钥。
    pub secret: String,
    /// 仓库 GIT_DIR 绝对路径。
    #[serde(default)]
    pub repo: String,
    /// 原始 refs 行(old new ref 每行一条)。
    #[serde(default)]
    pub refs: Vec<String>,
}

/// 内部端点:post-receive 钩子回调 → 发布 `commit.pushed`。
async fn post_receive(
    State(st): State<TransportState>,
    headers: HeaderMap,
    body: String,
) -> Result<Response, AppError> {
    let secret = headers
        .get("X-Hook-Secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if secret != st.hook_secret {
        return Err(AppError::forbidden(DOMAIN_CODE, "钩子密钥无效"));
    }
    let git_dir = headers
        .get("X-Hook-Repo")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let full_name = full_name_from_git_dir(git_dir, &st.repo_root);
    let refs: Vec<String> = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_owned)
        .collect();
    st.svc.publish_push(&full_name, &refs).await?;
    Ok((
        axum::http::StatusCode::OK,
        axum::Json(json!({"code": "OK"})),
    )
        .into_response())
}

/// 从 GIT_DIR 路径提取仓库全名。
fn full_name_from_git_dir(git_dir: &str, root: &str) -> String {
    let _ = root; // 路径可能相对/绝对不一致,改按 segments 中的 repositories 锚点提取
    let after = git_dir
        .rsplit_once("repositories/")
        .map(|(_, rest)| rest)
        .unwrap_or(git_dir);
    after
        .trim_start_matches('/')
        .strip_suffix(".git")
        .unwrap_or_default()
        .trim_matches('/')
        .to_owned()
}
