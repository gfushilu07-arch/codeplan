//! Git LFS(T-202):Batch API + basic transfer 适配器。
//!
//! - 端点挂在 smart HTTP 传输下:`/git/{owner}/{key}.git/info/lfs/*`,由
//!   `transport::git_proxy` 在 CGI 前分流(`parse_route`);
//! - 认证:Basic(密码位 = 会话 JWT + RBAC)或 SSH `git-lfs-authenticate`
//!   频发的短时 HMAC 令牌(`Authorization: RemoteAuth <token>`,绑定用户+仓库+操作);
//! - 存储:codeplan-storage(ObjectStore,内容寻址 `sha256/{oid}`)。

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use codeplan_common::error::AppError;
use codeplan_kernel::authz::{Action, ResourceRef};
use codeplan_kernel::identity::CurrentUser;
use codeplan_kernel::ids::RepoId;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use codeplan_storage::ObjectStore;

use crate::transport::TransportState;
use crate::{DOMAIN_CODE, domain::Repository};

/// LFS 子系统配置(store + 对外地址),server 构造,transport 持有。
#[derive(Clone)]
pub struct LfsState {
    /// LFS 对象存储。
    pub store: Arc<ObjectStore>,
    /// 对外基础地址(href 拼接)。
    pub external_url: String,
}

/// LFS over SSH 端点配置(令牌频发,server 构造,与钩子密钥同源)。
#[derive(Clone)]
pub struct EndpointConfig {
    /// 对外基础地址(href 拼接)。
    pub external_url: String,
    /// 令牌签名密钥。
    pub token_secret: String,
}

/// 构造 `git-lfs-authenticate` 响应体(客户端随后以
/// `Authorization: RemoteAuth <token>` 调用 HTTP batch 端点)。
#[must_use]
pub fn authenticate_json(
    cfg: &EndpointConfig,
    uid: &str,
    full_name: &str,
    op: &str,
) -> serde_json::Value {
    let token = mint_token(&cfg.token_secret, uid, full_name, op, TOKEN_TTL);
    let base = cfg.external_url.trim_end_matches('/');
    serde_json::json!({
        "href": format!("{base}/git/{full_name}.git/info/lfs"),
        "header": {"Authorization": format!("RemoteAuth {token}")},
        "expires_in": TOKEN_TTL.as_secs(),
    })
}

/// LFS 对象令牌有效期。
pub const TOKEN_TTL: Duration = Duration::from_secs(24 * 3600);

const LFS_JSON: &str = "application/vnd.git-lfs+json";
const MAX_BATCH_BODY: usize = 1024 * 1024;
const MAX_OBJECT_BODY: usize = 2 * 1024 * 1024 * 1024;

type HmacSha256 = Hmac<Sha256>;

/// LFS 内部辅助的错误响应(Response 较大,装箱满足 clippy result_large_err)。
type LfsReject = Box<Response>;

/// HMAC 签名(urlsafe base64,无填充)。
fn sign(secret: &str, msg: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac 接受任意长度密钥");
    mac.update(msg.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

/// 频发 LFS 访问令牌(绑定用户/仓库/操作,到期失效)。
#[must_use]
pub fn mint_token(secret: &str, uid: &str, full_name: &str, op: &str, ttl: Duration) -> String {
    let exp = chrono::Utc::now().timestamp() + ttl.as_secs() as i64;
    let msg = format!("{uid}:{op}:{exp}:{full_name}");
    format!("{msg}:{}", sign(secret, &msg))
}

/// 校验令牌;返回 (uid, op, full_name)。
pub fn verify_token(secret: &str, token: &str) -> Option<(String, String, String)> {
    let (msg, sig) = token.rsplit_once(':')?;
    if sign(secret, msg) != sig {
        return None;
    }
    let mut parts = msg.split(':');
    let uid = parts.next()?;
    let op = parts.next()?;
    let exp: i64 = parts.next()?.parse().ok()?;
    let full_name = parts.next()?;
    if parts.next().is_some() || chrono::Utc::now().timestamp() >= exp {
        return None;
    }
    Some((uid.to_owned(), op.to_owned(), full_name.to_owned()))
}

/// LFS 路由(从 smart HTTP rest 路径解析)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// POST `info/lfs/objects/batch`。
    Batch,
    /// PUT/GET `info/lfs/objects/{oid}`。
    Object(String),
    /// POST `info/lfs/verify`。
    Verify,
}

/// 解析 rest 路径为 (仓库全名, 路由);非 LFS 路径返回 None。
pub fn parse_route(rest: &str) -> Option<(String, Route)> {
    let mut segs = rest.split('/').filter(|s| !s.is_empty());
    let owner = segs.next()?;
    let key_git = segs.next()?;
    let key = key_git.strip_suffix(".git")?;
    let route = match segs.collect::<Vec<_>>().as_slice() {
        ["info", "lfs", "objects", "batch"] => Route::Batch,
        ["info", "lfs", "verify"] => Route::Verify,
        ["info", "lfs", "objects", oid] => Route::Object((*oid).to_owned()),
        _ => return None,
    };
    Some((format!("{owner}/{key}"), route))
}

/// 入口(smart HTTP 代理在 CGI 前分流;仓库解析与归档检查已在代理完成)。
pub async fn handle(
    st: TransportState,
    lfs: LfsState,
    repo: Repository,
    req: Request<Body>,
) -> Result<Response, AppError> {
    let (parts, body) = req.into_parts();
    let method = parts.method.clone();
    let rest = parts
        .uri
        .path()
        .strip_prefix("/git/")
        .unwrap_or_default()
        .to_owned();
    let Some((full_name, route)) = parse_route(&rest) else {
        return Err(AppError::not_found(DOMAIN_CODE, "LFS 路径非法"));
    };
    let actor = match authorize(&st, &parts.headers, &full_name).await {
        Ok(a) => a,
        Err(resp) => return Ok(*resp),
    };

    match route {
        Route::Batch => batch(&st, &lfs, method, body, full_name, &repo, &actor).await,
        Route::Object(oid) => {
            if method == Method::PUT {
                upload(&st, &lfs, body, &repo, &actor, oid).await
            } else {
                download(&st, &lfs, &repo, &actor, oid).await
            }
        }
        Route::Verify => verify(&st, &lfs, body, &repo, &actor).await,
    }
}

#[derive(Debug, Deserialize)]
struct BatchRequest {
    operation: String,
    #[serde(default)]
    objects: Vec<LfsObjectIn>,
}

#[derive(Debug, Deserialize)]
struct LfsObjectIn {
    oid: String,
    size: i64,
}

/// 认证结果。
enum Actor {
    User(CurrentUser),
    /// (uid, op)
    Token(String, String),
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            header::HeaderValue::from_static("Basic realm=\"codeplan\""),
        )],
    )
        .into_response()
}

/// Basic(JWT)或 RemoteAuth(短时令牌,校验仓库绑定)。
async fn authorize(
    st: &TransportState,
    headers: &HeaderMap,
    full_name: &str,
) -> Result<Actor, LfsReject> {
    let Some(raw) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return Err(Box::new(unauthorized()));
    };
    if let Some(token) = raw.strip_prefix("Basic ") {
        let Some(text) = base64::engine::general_purpose::STANDARD
            .decode(token.trim())
            .ok()
            .and_then(|d| String::from_utf8(d).ok())
        else {
            return Err(Box::new(unauthorized()));
        };
        let Some((_, jwt)) = text.split_once(':') else {
            return Err(Box::new(unauthorized()));
        };
        let Ok(user) = st.authn.verify(jwt).await else {
            return Err(Box::new(unauthorized()));
        };
        return Ok(Actor::User(user));
    }
    if let Some(token) = raw.strip_prefix("RemoteAuth ") {
        return match verify_token(&st.hook_secret, token.trim()) {
            Some((uid, op, name)) if name == full_name => Ok(Actor::Token(uid, op)),
            _ => Err(Box::new(
                (StatusCode::FORBIDDEN, "LFS 令牌无效或已过期".to_owned()).into_response(),
            )),
        };
    }
    Err(Box::new(unauthorized()))
}

/// 检查操作权限:Token 校验 op 匹配(铸造时已 RBAC);User 由 ensure_rbac 处理。
async fn ensure_op(actor: &Actor, repo: &Repository, action: Action) -> Result<(), LfsReject> {
    let _ = repo;
    match actor {
        Actor::Token(_, op) => {
            let allowed = match action {
                Action::Write => op == "upload",
                Action::Read => op == "download",
                Action::Admin => false,
            };
            if allowed {
                Ok(())
            } else {
                Err(Box::new(
                    (StatusCode::FORBIDDEN, "LFS 令牌操作类型不匹配".to_owned()).into_response(),
                ))
            }
        }
        Actor::User(u) => {
            let scope = match action {
                Action::Write => "repo:write",
                _ => "repo:read",
            };
            if u.has_scope(scope) || u.has_scope("api") {
                Ok(())
            } else {
                Err(Box::new(
                    (StatusCode::FORBIDDEN, format!("PAT 缺少 {scope} scope")).into_response(),
                ))
            }
        }
    }
}

/// Basic 用户的 RBAC 校验(Token 已在铸造时校验)。
async fn ensure_rbac(
    st: &TransportState,
    actor: &Actor,
    repo: &Repository,
    action: Action,
) -> Result<(), LfsReject> {
    if let Actor::User(user) = actor {
        let reference = ResourceRef::Repo(RepoId::from_uuid(repo.id.as_uuid()));
        let ok = st.svc.ensure_action(user, action, &reference).await.is_ok();
        if !ok {
            return Err(Box::new(unauthorized()));
        }
    }
    Ok(())
}

fn lfs_headers() -> [(header::HeaderName, &'static str); 1] {
    [(header::CONTENT_TYPE, LFS_JSON)]
}

fn actor_uid(actor: &Actor) -> String {
    match actor {
        Actor::User(u) => u.id.to_string(),
        Actor::Token(uid, _) => uid.clone(),
    }
}

/// `POST objects/batch`:为每个对象签发带短时令牌的 href。
async fn batch(
    st: &TransportState,
    lfs: &LfsState,
    method: Method,
    body: Body,
    full_name: String,
    repo: &Repository,
    actor: &Actor,
) -> Result<Response, AppError> {
    if method != Method::POST {
        return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
    }
    let Ok(body) = axum::body::to_bytes(body, MAX_BATCH_BODY).await else {
        return Err(AppError::invalid(DOMAIN_CODE, "batch 请求体过大"));
    };
    let Ok(parsed) = serde_json::from_slice::<BatchRequest>(&body) else {
        return Err(AppError::invalid(DOMAIN_CODE, "batch 请求体非法"));
    };

    let action = if parsed.operation == "upload" {
        Action::Write
    } else {
        Action::Read
    };
    if ensure_op(actor, repo, action).await.is_err() {
        return Ok((StatusCode::FORBIDDEN, "LFS 令牌操作类型不匹配").into_response());
    }
    if ensure_rbac(st, actor, repo, action).await.is_err() {
        return Ok(unauthorized());
    }

    let base = lfs.external_url.trim_end_matches('/');
    let uid = actor_uid(actor);
    let mut objects = Vec::with_capacity(parsed.objects.len());
    for obj in &parsed.objects {
        let key = format!("sha256/{}", obj.oid);
        if parsed.operation == "upload" {
            let token = mint_token(&st.hook_secret, &uid, &full_name, "upload", TOKEN_TTL);
            let header = json!({"Authorization": format!("RemoteAuth {token}")});
            objects.push(json!({
                "oid": obj.oid,
                "size": obj.size,
                "authenticated": true,
                "actions": {
                    "upload": {
                        "href": format!("{base}/git/{full_name}.git/info/lfs/objects/{oid}", oid = obj.oid),
                        "header": header,
                        "expires_in": TOKEN_TTL.as_secs(),
                    },
                    "verify": {
                        "href": format!("{base}/git/{full_name}.git/info/lfs/verify"),
                        "header": json!({"Authorization": format!("RemoteAuth {}", mint_token(&st.hook_secret, &uid, &full_name, "upload", TOKEN_TTL))}),
                        "expires_in": TOKEN_TTL.as_secs(),
                    },
                },
            }));
        } else {
            match lfs.store.size(&key).await {
                Ok(Some(_)) => {
                    let token =
                        mint_token(&st.hook_secret, &uid, &full_name, "download", TOKEN_TTL);
                    objects.push(json!({
                        "oid": obj.oid,
                        "size": obj.size,
                        "authenticated": true,
                        "actions": {
                            "download": {
                                "href": format!("{base}/git/{full_name}.git/info/lfs/objects/{oid}", oid = obj.oid),
                                "header": {"Authorization": format!("RemoteAuth {token}")},
                                "expires_in": TOKEN_TTL.as_secs(),
                            },
                        },
                    }));
                }
                Ok(None) => objects.push(json!({
                    "oid": obj.oid,
                    "size": obj.size,
                    "error": {"code": 404, "message": "对象不存在"},
                })),
                Err(e) => {
                    return Err(AppError::internal(DOMAIN_CODE).with_details(json!(e.to_string())));
                }
            }
        }
    }

    Ok((
        lfs_headers(),
        axum::Json(json!({
            "transfer": "basic",
            "objects": objects,
            "hash_algo": "sha256",
        })),
    )
        .into_response())
}

/// `PUT objects/{oid}`:校验 sha256 后转存对象存储。
async fn upload(
    st: &TransportState,
    lfs: &LfsState,
    body: Body,
    repo: &Repository,
    actor: &Actor,
    oid: String,
) -> Result<Response, AppError> {
    if ensure_op(actor, repo, Action::Write).await.is_err() {
        return Ok((StatusCode::FORBIDDEN, "LFS 令牌操作类型不匹配").into_response());
    }
    if ensure_rbac(st, actor, repo, Action::Write).await.is_err() {
        return Ok(unauthorized());
    }
    if oid.len() != 64 || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(AppError::invalid(DOMAIN_CODE, "oid 非法"));
    }

    let Ok(data) = axum::body::to_bytes(body, MAX_OBJECT_BODY).await else {
        return Err(AppError::invalid(DOMAIN_CODE, "对象体积超限"));
    };
    let data = data.to_vec();
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let digest: String = hasher.finalize().iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
        s
    });
    if !digest.eq_ignore_ascii_case(&oid) {
        return Err(AppError::invalid(
            DOMAIN_CODE,
            format!("内容哈希 {digest} 与 oid 不符"),
        ));
    }
    lfs.store
        .put(&format!("sha256/{oid}"), data)
        .await
        .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(json!(e.to_string())))?;
    Ok((StatusCode::OK, lfs_headers()).into_response())
}

/// `GET objects/{oid}`:回源对象内容。
async fn download(
    st: &TransportState,
    lfs: &LfsState,
    repo: &Repository,
    actor: &Actor,
    oid: String,
) -> Result<Response, AppError> {
    if ensure_op(actor, repo, Action::Read).await.is_err() {
        return Ok((StatusCode::FORBIDDEN, "LFS 令牌操作类型不匹配").into_response());
    }
    if ensure_rbac(st, actor, repo, Action::Read).await.is_err() {
        return Ok(unauthorized());
    }
    match lfs.store.get(&format!("sha256/{oid}")).await {
        Ok(Some(data)) => Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/octet-stream")],
            data,
        )
            .into_response()),
        Ok(None) => Ok(StatusCode::NOT_FOUND.into_response()),
        Err(e) => Err(AppError::internal(DOMAIN_CODE).with_details(json!(e.to_string()))),
    }
}

/// `POST verify`:核对对象存在且大小一致。
async fn verify(
    st: &TransportState,
    lfs: &LfsState,
    body: Body,
    repo: &Repository,
    actor: &Actor,
) -> Result<Response, AppError> {
    if ensure_op(actor, repo, Action::Write).await.is_err() {
        return Ok((StatusCode::FORBIDDEN, "LFS 令牌操作类型不匹配").into_response());
    }
    if ensure_rbac(st, actor, repo, Action::Write).await.is_err() {
        return Ok(unauthorized());
    }
    #[derive(Deserialize)]
    struct VerifyReq {
        oid: String,
        size: i64,
    }
    let Ok(body) = axum::body::to_bytes(body, MAX_BATCH_BODY).await else {
        return Err(AppError::invalid(DOMAIN_CODE, "verify 请求体过大"));
    };
    let Ok(req_json) = serde_json::from_slice::<VerifyReq>(&body) else {
        return Err(AppError::invalid(DOMAIN_CODE, "verify 请求体非法"));
    };
    match lfs.store.size(&format!("sha256/{}", req_json.oid)).await {
        Ok(Some(actual)) if actual == req_json.size as u64 => {
            Ok((StatusCode::OK, lfs_headers()).into_response())
        }
        Ok(Some(_)) => Ok(StatusCode::BAD_REQUEST.into_response()),
        _ => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "test-secret";

    #[test]
    fn token_roundtrip() {
        let token = mint_token(SECRET, "u-1", "admin/demo", "upload", TOKEN_TTL);
        let (uid, op, name) = verify_token(SECRET, &token).expect("应通过校验");
        assert_eq!(
            (uid.as_str(), op.as_str(), name.as_str()),
            ("u-1", "upload", "admin/demo")
        );
    }

    #[test]
    fn token_rejects_wrong_secret_and_expiry() {
        let token = mint_token(SECRET, "u-1", "admin/demo", "upload", TOKEN_TTL);
        assert!(verify_token("other-secret", &token).is_none(), "密钥不符");

        // ttl=0 → exp=now,verify 用 >= 判过期
        let expired = mint_token(SECRET, "u-1", "admin/demo", "upload", Duration::ZERO);
        assert!(verify_token(SECRET, &expired).is_none(), "应视为已过期");
    }

    #[test]
    fn token_rejects_repo_mismatch() {
        let token = mint_token(SECRET, "u-1", "admin/demo", "upload", TOKEN_TTL);
        // 令牌绑定 admin/demo,不能用于 admin/other
        let (_, _, name) = verify_token(SECRET, &token).unwrap();
        assert_ne!(name, "admin/other");
    }

    #[test]
    fn parse_route_variants() {
        assert_eq!(
            parse_route("admin/demo.git/info/lfs/objects/batch"),
            Some(("admin/demo".into(), Route::Batch))
        );
        assert_eq!(
            parse_route("admin/demo.git/info/lfs/objects/abc123"),
            Some(("admin/demo".into(), Route::Object("abc123".into())))
        );
        assert_eq!(
            parse_route("admin/demo.git/info/lfs/verify"),
            Some(("admin/demo".into(), Route::Verify))
        );
        assert_eq!(parse_route("admin/demo.git/info/refs"), None);
        assert_eq!(parse_route("admin/demo.git/info/lfs/other"), None);
    }
}
