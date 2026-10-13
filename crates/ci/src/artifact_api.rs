//! 制品库协议端点(T-304):Maven layout / npm registry / PyPI(twine)/
//! OCI distribution v2 最小子集 + 通用 REST。认证:Basic(JWT 密码位)
//! 或 Bearer;仓库路由约定 `{owner}/{key}` 取 URL 前两段。

use codeplan_common::error::AppError;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use base64::Engine as _;
use codeplan_kernel::authn::TokenAuthenticator;
use codeplan_kernel::ids::RepoId;
use serde_json::json;
use sha2::Digest as _;

use crate::artifact_store::ArtifactStore;

/// 协议状态。
#[derive(Clone)]
pub struct RegistryState {
    /// 存储门面。
    pub store: std::sync::Arc<ArtifactStore>,
    /// 认证器(Basic 密码位 = JWT)。
    pub authn: std::sync::Arc<dyn TokenAuthenticator>,
    /// 仓库根(OCI 挂载/定位展示用,可空)。
    pub repo_hint: String,
}

/// 路由:`/maven/**`、`/npm/**`、`/pypi/**`、`/v2/**`、`/artifacts/**`。
pub fn routes(state: RegistryState) -> Router<RegistryState> {
    Router::new()
        .route("/maven/{*rest}", any(maven_handler))
        .route("/npm/{pkg}", axum::routing::any(npm_package))
        .route("/npm/{pkg}/-/{file}", get(npm_tarball))
        .route("/pypi/", axum::routing::post(pypi_upload))
        .route("/pypi/{name}/", get(pypi_index))
        .route("/pypi/{name}/-/{file}", get(pypi_file))
        .route("/v2/", get(v2_ping))
        .route(
            "/v2/{name}/manifests/{reference}",
            get(v2_manifest).put(v2_put_manifest),
        )
        .route("/v2/{name}/blobs/{digest}", get(v2_blob).put(v2_put_blob))
        .route(
            "/artifacts/{owner}/{key}",
            get(generic_list).put(generic_put),
        )
        .route(
            "/artifacts/{owner}/{key}/{kind}/{name}/{version}/{file}",
            get(generic_get),
        )
        .with_state(state)
}

use axum::extract::Path;

// ---------- 认证与仓库定位 ----------

fn unauthorized() -> Box<Response> {
    Box::new(
        (
            StatusCode::UNAUTHORIZED,
            [(
                header::WWW_AUTHENTICATE,
                header::HeaderValue::from_static("Basic realm=\"codeplan-registry\""),
            )],
        )
            .into_response(),
    )
}

async fn authenticate(
    st: &RegistryState,
    headers: &HeaderMap,
) -> Result<codeplan_kernel::identity::CurrentUser, Box<Response>> {
    let raw = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(unauthorized)?;
    let token = if let Some(b64) = raw.strip_prefix("Basic ") {
        let text = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .ok()
            .and_then(|d| String::from_utf8(d).ok())
            .ok_or_else(unauthorized)?;
        text.split_once(':')
            .map(|(_, pw)| pw.to_owned())
            .ok_or_else(unauthorized)?
    } else if let Some(bearer) = raw.strip_prefix("Bearer ") {
        bearer.to_owned()
    } else {
        return Err(unauthorized());
    };
    st.authn.verify(&token).await.map_err(|_| unauthorized())
}

async fn repo_id_of(st: &RegistryState, owner: &str, key: &str) -> Result<RepoId, Box<Response>> {
    // RegistryState 没有 pool:复用 ArtifactStore 内 pool?给 store 加 pool 访问器
    st.store
        .repo_id_by_full_name(&format!("{owner}/{key}"))
        .await
        .map_err(|_| Box::new(StatusCode::INTERNAL_SERVER_ERROR.into_response()))?
        .ok_or_else(|| Box::new(StatusCode::NOT_FOUND.into_response()))
}

fn sha256_hex(data: &[u8]) -> String {
    let digest = sha2::Sha256::digest(data);
    digest.iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
        s
    })
}

// ---------- Maven layout:/{group/path}/{artifact}/{version}/{file} ----------

async fn maven_handler(
    State(st): State<RegistryState>,
    req: Request<Body>,
) -> Result<Response, AppError> {
    let (parts, body) = req.into_parts();
    let path = parts.uri.path().trim_start_matches("/maven/");
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() < 4 {
        return Err(AppError::invalid(
            "REG",
            "Maven 路径需 {group…}/{artifact}/{version}/{file}",
        ));
    }
    let version = segs[segs.len() - 2];
    let filename = segs[segs.len() - 1];
    let artifact = segs[segs.len() - 3];
    let group = segs[..segs.len() - 3].join(".");
    let name = format!("{group}:{artifact}");
    let (owner, key) = ("admin", "maven"); // v1:单内置仓库(env CODEPLAN_MAVEN_REPO 可扩)
    let _ = (owner, key);
    let repo_id = RepoId::from_uuid(uuid::Uuid::nil()); // 占位:独立协议仓库空间
    let _ = repo_id;
    // 与通用 REST 共库:repo_id 用 nil 表示“注册表级”(跨仓库公共)
    let repo_id = RepoId::from_uuid(uuid::Uuid::nil());

    match parts.method.as_str() {
        "PUT" => {
            if let Err(resp) = authenticate(&st, &parts.headers).await {
                return Ok(*resp);
            }
            let data = axum::body::to_bytes(body, 512 * 1024 * 1024)
                .await
                .map_err(|e| AppError::invalid("REG", format!("读取失败: {e}")))?
                .to_vec();
            st.store
                .put(
                    repo_id,
                    "maven",
                    &name,
                    version,
                    filename,
                    data,
                    json!({}),
                    None,
                )
                .await?;
            Ok(StatusCode::CREATED.into_response())
        }
        "GET" | "HEAD" => {
            let Some(data) = st
                .store
                .content(repo_id, "maven", &name, version, filename)
                .await?
            else {
                return Ok(StatusCode::NOT_FOUND.into_response());
            };
            let _ = authenticate(&st, &parts.headers).await;
            Ok(([(header::CONTENT_TYPE, "application/octet-stream")], data).into_response())
        }
        _ => Ok(StatusCode::METHOD_NOT_ALLOWED.into_response()),
    }
}

// ---------- npm registry ----------

async fn npm_package(
    State(st): State<RegistryState>,
    Path(pkg): Path<String>,
    req: Request<Body>,
) -> Result<Response, AppError> {
    let (parts, body) = req.into_parts();
    let repo_id = RepoId::from_uuid(uuid::Uuid::nil());
    match parts.method.as_str() {
        "PUT" => {
            let user = match authenticate(&st, &parts.headers).await {
                Ok(u) => u,
                Err(resp) => return Ok(*resp),
            };
            let data = axum::body::to_bytes(body, 512 * 1024 * 1024)
                .await
                .map_err(|e| AppError::invalid("REG", format!("读取失败: {e}")))?
                .to_vec();
            // npm publish 的 PUT body:{"_attachments": {"file": {"data": base64, …}}, "versions": {...}}
            let payload: serde_json::Value = serde_json::from_slice(&data)
                .map_err(|e| AppError::invalid("REG", format!("npm 包体非法: {e}")))?;
            let attachments = payload
                .pointer("/_attachments")
                .and_then(|a| a.as_object())
                .ok_or_else(|| AppError::invalid("REG", "缺少 _attachments"))?;
            let mut stored = Vec::new();
            for (key, att) in attachments {
                // key 形如 pkg-name/-/pkg-name-1.0.0.tgz
                let filename = key.rsplit('/').next().unwrap_or(key).to_owned();
                let Some(b64) = att.pointer("/data").and_then(|d| d.as_str()) else {
                    continue;
                };
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| AppError::invalid("REG", format!("附件 base64 非法: {e}")))?;
                let version = filename
                    .strip_prefix(&format!("{pkg}-"))
                    .and_then(|s| s.strip_suffix(".tgz"))
                    .unwrap_or("0.0.0")
                    .to_owned();
                st.store
                    .put(
                        repo_id,
                        "npm",
                        &pkg,
                        &version,
                        &filename,
                        bytes,
                        json!({}),
                        Some(user.id.as_uuid()),
                    )
                    .await?;
                stored.push((version, filename));
            }
            if stored.is_empty() {
                return Err(AppError::invalid("REG", "无有效附件"));
            }
            Ok((StatusCode::OK, Json(json!({"ok": true, "stored": stored}))).into_response())
        }
        "GET" => {
            let versions = st.store.versions(repo_id, "npm", &pkg).await?;
            if versions.is_empty() {
                return Ok(StatusCode::NOT_FOUND.into_response());
            }
            // 简化 packument:dist-tags.latest = 最大版本(字典序);tarball URL 指向本服务
            let mut versions_obj = serde_json::Map::new();
            let mut latest: Option<&crate::artifact_store::Artifact> = None;
            for v in &versions {
                let tarball = format!("/npm/{}/-/{}/{}", pkg, pkg, v.filename);
                versions_obj.insert(
                    v.version.clone(),
                    json!({
                        "name": pkg,
                        "version": v.version,
                        "dist": {"tarball": tarball},
                    }),
                );
                if latest
                    .map(|l| l.version.as_str() < v.version.as_str())
                    .unwrap_or(true)
                {
                    latest = Some(v);
                }
            }
            let dist_tag = latest
                .map(|l| json!({"latest": l.version.clone()}))
                .unwrap_or(json!({}));
            Ok(Json(json!({
                "name": pkg,
                "dist-tags": dist_tag,
                "versions": versions_obj,
            }))
            .into_response())
        }
        _ => Ok(StatusCode::METHOD_NOT_ALLOWED.into_response()),
    }
}

async fn npm_tarball(
    State(st): State<RegistryState>,
    Path((pkg, file)): Path<(String, String)>,
    _headers: HeaderMap,
) -> Result<Response, AppError> {
    let repo_id = RepoId::from_uuid(uuid::Uuid::nil());
    let versions = st.store.versions(repo_id, "npm", &pkg).await?;
    let Some(v) = versions.into_iter().find(|a| a.filename == file) else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    match st
        .store
        .content(repo_id, "npm", &pkg, &v.version, &file)
        .await?
    {
        Some(data) => Ok(([(header::CONTENT_TYPE, "application/x-tgz")], data).into_response()),
        None => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

// ---------- PyPI(PEP 503 简版;twine multipart 上传) ----------

async fn pypi_upload(
    State(st): State<RegistryState>,
    req: Request<Body>,
) -> Result<Response, AppError> {
    let (parts, body) = req.into_parts();
    let user = match authenticate(&st, &parts.headers).await {
        Ok(u) => u,
        Err(resp) => return Ok(*resp),
    };
    let data = axum::body::to_bytes(body, 512 * 1024 * 1024)
        .await
        .map_err(|e| AppError::invalid("REG", format!("读取失败: {e}")))?
        .to_vec();
    // 解析 multipart:name/version/content(filename)
    let text = String::from_utf8_lossy(&data);
    let boundary = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|ct| ct.split("boundary=").nth(1))
        .map(str::to_owned)
        .ok_or_else(|| AppError::invalid("REG", "缺少 multipart boundary"))?;
    let name = multipart_field(&text, &boundary, "name")
        .ok_or_else(|| AppError::invalid("REG", "缺少 name"))?;
    let version = multipart_field(&text, &boundary, "version")
        .ok_or_else(|| AppError::invalid("REG", "缺少 version"))?;
    // 取第一个文件段
    let (filename, content) =
        multipart_file(&text, &boundary).ok_or_else(|| AppError::invalid("REG", "缺少文件段"))?;

    // multipart 文本解析在二进制(jar/tar)上会截断 — 此处按字节解析重做
    let raw_content = multipart_file_bytes(&data, &boundary)
        .ok_or_else(|| AppError::invalid("REG", "文件段解析失败"))?;
    let _ = content;

    let repo_id = RepoId::from_uuid(uuid::Uuid::nil());
    st.store
        .put(
            repo_id,
            "pypi",
            &name,
            &version,
            &filename,
            raw_content,
            json!({}),
            Some(user.id.as_uuid()),
        )
        .await?;
    Ok(StatusCode::OK.into_response())
}

fn multipart_field(text: &str, boundary: &str, field: &str) -> Option<String> {
    let marker = format!("name=\"{field}\"");
    let idx = text.find(&marker)?;
    let after = &text[idx + marker.len()..];
    let start = after.find("\r\n\r\n")? + 4;
    let end = after[start..].find(&format!("--{boundary}"))? + start;
    Some(after[start..end].trim_end_matches("\r\n").to_owned())
}

fn multipart_file(text: &str, boundary: &str) -> Option<(String, String)> {
    let marker = "filename=\"";
    let mut search = 0;
    while let Some(pos) = text[search..].find(marker) {
        let abs = search + pos + marker.len();
        let end = text[abs..].find('"')? + abs;
        let filename = text[abs..end].to_owned();
        let body_start = text[end..].find("\r\n\r\n")? + end + 4;
        let body_end = text[body_start..].find(&format!("--{boundary}"))? + body_start;
        let content_len = text[body_start..body_end].trim_end_matches("\r\n").len();
        if content_len > 0 {
            return Some((
                filename,
                text[body_start..body_start + content_len].to_owned(),
            ));
        }
        search = body_end;
    }
    None
}

fn multipart_file_bytes(data: &[u8], boundary: &str) -> Option<Vec<u8>> {
    let marker = b"filename=\"";
    let pos = data.windows(marker.len()).position(|w| w == marker)?;
    let after = &data[pos + marker.len()..];
    let quote = after.iter().position(|b| *b == b'"')?;
    let rest = &after[quote + 1..];
    let sep = b"\r\n\r\n";
    let sep_pos = rest.windows(sep.len()).position(|w| w == sep)?;
    let body = &rest[sep_pos + sep.len()..];
    let end_marker = format!("--{boundary}");
    let end = body
        .windows(end_marker.len())
        .position(|w| w == end_marker.as_bytes())?;
    Some(body[..body[end..].iter().position(|b| *b == b'\r').unwrap_or(end)].to_vec())
}

async fn pypi_index(
    State(st): State<RegistryState>,
    Path(name): Path<String>,
) -> Result<Response, AppError> {
    let repo_id = RepoId::from_uuid(uuid::Uuid::nil());
    let versions = st.store.versions(repo_id, "pypi", &name).await?;
    if versions.is_empty() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let mut links = String::new();
    for v in &versions {
        let hash = &v.sha256[..v.sha256.len().min(16)];
        links.push_str(&format!(
            "<a href=\"/pypi/{name}/-/{file}#{hash}\">{file}</a><br>",
            file = v.filename
        ));
    }
    Ok((
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        format!("<html><body><h1>{name}</h1>{links}</body></html>"),
    )
        .into_response())
}

async fn pypi_file(
    State(st): State<RegistryState>,
    Path((name, file)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let repo_id = RepoId::from_uuid(uuid::Uuid::nil());
    let versions = st.store.versions(repo_id, "pypi", &name).await?;
    let Some(v) = versions.into_iter().find(|a| a.filename == file) else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    match st
        .store
        .content(repo_id, "pypi", &name, &v.version, &file)
        .await?
    {
        Some(data) => {
            Ok(([(header::CONTENT_TYPE, "application/octet-stream")], data).into_response())
        }
        None => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

// ---------- OCI distribution v2 最小子集 ----------

async fn v2_ping() -> Response {
    Json(json!({})).into_response()
}

async fn v2_put_manifest(
    State(st): State<RegistryState>,
    Path((name, reference)): Path<(String, String)>,
    req: Request<Body>,
) -> Result<Response, AppError> {
    let (parts, body) = req.into_parts();
    if let Err(resp) = authenticate(&st, &parts.headers).await {
        return Ok(*resp);
    }
    let data = axum::body::to_bytes(body, 64 * 1024 * 1024)
        .await
        .map_err(|e| AppError::invalid("REG", format!("读取失败: {e}")))?
        .to_vec();
    let digest = format!("sha256:{}", sha256_hex(&data));
    let repo_id = RepoId::from_uuid(uuid::Uuid::nil());
    st.store
        .put(
            repo_id,
            "oci",
            name.trim(),
            &reference,
            &digest,
            data,
            json!({}),
            None,
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        [(
            header::ETAG,
            header::HeaderValue::from_str(&digest)
                .unwrap_or_else(|_| header::HeaderValue::from_static("sha256:?")),
        )],
    )
        .into_response())
}

async fn v2_manifest(
    State(st): State<RegistryState>,
    Path((name, reference)): Path<(String, String)>,
    req: Request<Body>,
) -> Result<Response, AppError> {
    let _ = authenticate(&st, req.headers()).await;
    let repo_id = RepoId::from_uuid(uuid::Uuid::nil());
    // reference 可能是 digest 或 tag:tag → manifest 文件名即 reference;digest → filename=digest
    let (version, filename) = if let Some(d) = reference.strip_prefix("sha256:") {
        (format!("sha256:{d}"), format!("sha256:{d}"))
    } else {
        (reference.clone(), reference)
    };
    let Some(data) = st
        .store
        .content(repo_id, "oci", name.trim(), &version, &filename)
        .await?
    else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let etag = header::HeaderValue::from_str(&format!("sha256:{}", sha256_hex(&data)))
        .unwrap_or_else(|_| header::HeaderValue::from_static("sha256:invalid"));
    Ok((
        [
            (
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("application/vnd.oci.image.manifest.v1+json"),
            ),
            (header::ETAG, etag),
        ],
        data,
    )
        .into_response())
}

async fn v2_blob(
    State(st): State<RegistryState>,
    Path((name, digest)): Path<(String, String)>,
    req: Request<Body>,
) -> Result<Response, AppError> {
    let _ = authenticate(&st, req.headers()).await;
    let repo_id = RepoId::from_uuid(uuid::Uuid::nil());
    match st
        .store
        .content(repo_id, "oci", name.trim(), &digest, &digest)
        .await?
    {
        Some(data) => {
            Ok(([(header::CONTENT_TYPE, "application/octet-stream")], data).into_response())
        }
        None => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

// ---------- 通用 REST(Bearer/Basic) ----------

async fn generic_list(
    State(st): State<RegistryState>,
    Path((owner, key)): Path<(String, String)>,
    req: Request<Body>,
) -> Result<Response, AppError> {
    if authenticate(&st, req.headers()).await.is_err() {
        return Ok(*unauthorized());
    }
    let repo_id = match repo_id_of(&st, &owner, &key).await {
        Ok(id) => id,
        Err(resp) => return Ok(*resp),
    };
    // 全 kind 列表:kind 遍历(简单实现:按 kind 查询合并)
    let mut all = Vec::new();
    for kind in ["maven", "npm", "pypi", "oci", "generic", "pipeline"] {
        // name="*" 不支持 — 用 versions 全表扫描太贵;v1 只列 pipeline 产物:
        if kind == "pipeline" {
            let rows = st.store.versions(repo_id, "pipeline", "dist").await?;
            for a in rows {
                all.push(a);
            }
        }
    }
    Ok(Json(json!({"code": "OK", "data": all})).into_response())
}

async fn generic_put(
    State(st): State<RegistryState>,
    Path((owner, key)): Path<(String, String)>,
    req: Request<Body>,
) -> Result<Response, AppError> {
    let (parts, body) = req.into_parts();
    let user = match authenticate(&st, &parts.headers).await {
        Ok(u) => u,
        Err(resp) => return Ok(*resp),
    };
    let repo_id = match repo_id_of(&st, &owner, &key).await {
        Ok(id) => id,
        Err(resp) => return Ok(*resp),
    };
    // 查询串解析(axum Query 放这层避免重复 derive)
    let qs = parts.uri.query().unwrap_or_default();
    let mut name = String::new();
    let mut version = String::new();
    let mut file = String::new();
    for pair in qs.split('&') {
        let mut kv = pair.split('=');
        match kv.next() {
            Some("name") => name = kv.next().unwrap_or_default().to_owned(),
            Some("version") => version = kv.next().unwrap_or_default().to_owned(),
            Some("filename") => file = kv.next().unwrap_or_default().to_owned(),
            _ => {}
        }
    }
    if name.is_empty() || version.is_empty() || file.is_empty() {
        return Err(AppError::invalid("REG", "需要 name/version/filename"));
    }
    let data = axum::body::to_bytes(body, 512 * 1024 * 1024)
        .await
        .map_err(|e| AppError::invalid("REG", format!("读取失败: {e}")))?
        .to_vec();
    let artifact = st
        .store
        .put(
            repo_id,
            "pipeline",
            &name,
            &version,
            &file,
            data,
            json!({}),
            Some(user.id.as_uuid()),
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"code": "OK", "data": artifact})),
    )
        .into_response())
}

async fn generic_get(
    State(st): State<RegistryState>,
    Path((owner, key, kind, name, version, file)): Path<(
        String,
        String,
        String,
        String,
        String,
        String,
    )>,
    req: Request<Body>,
) -> Result<Response, AppError> {
    if authenticate(&st, req.headers()).await.is_err() {
        return Ok(*unauthorized());
    }
    let repo_id = match repo_id_of(&st, &owner, &key).await {
        Ok(id) => id,
        Err(resp) => return Ok(*resp),
    };
    let Some(data) = st
        .store
        .content(
            repo_id,
            kind.as_str(),
            name.as_str(),
            version.as_str(),
            file.as_str(),
        )
        .await?
    else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], data).into_response())
}

/// OCI blob 直传(monolithic POST 语义的 PUT 变体,digest 即内容寻址)。
async fn v2_put_blob(
    State(st): State<RegistryState>,
    Path((name, digest)): Path<(String, String)>,
    req: Request<Body>,
) -> Result<Response, AppError> {
    let (parts, body) = req.into_parts();
    if let Err(resp) = authenticate(&st, &parts.headers).await {
        return Ok(*resp);
    }
    let data = axum::body::to_bytes(body, 512 * 1024 * 1024)
        .await
        .map_err(|e| AppError::invalid("REG", format!("读取失败: {e}")))?
        .to_vec();
    let actual = format!("sha256:{}", sha256_hex(&data));
    if actual != digest {
        return Err(AppError::invalid(
            "REG",
            format!("digest 不符: {actual} != {digest}"),
        ));
    }
    let repo_id = RepoId::from_uuid(uuid::Uuid::nil());
    st.store
        .put(
            repo_id,
            "oci",
            name.trim(),
            &digest,
            &digest,
            data,
            json!({}),
            None,
        )
        .await?;
    Ok(StatusCode::CREATED.into_response())
}
