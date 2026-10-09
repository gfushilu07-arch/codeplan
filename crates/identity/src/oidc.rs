//! OIDC 单点登录(ORG-02):配置化启用——任一环境变量缺失即视为未启用(路由 404)。
//!
//! 流程:`/api/auth/oidc/login` 302 到 IdP(state=签名 JWT 内含 nonce)→ IdP 回调 →
//! 校验 state/nonce → 按邮箱 JIT 开通用户 → 302 回前端 `/#/auth/callback?token=<JWT>`。

use axum::Router;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use codeplan_common::error::AppError;
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::reqwest;
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, IssuerUrl, Nonce, RedirectUrl, Scope, TokenResponse,
};
use serde::Deserialize;
use tokio::sync::OnceCell;

use crate::DOMAIN_CODE;
use crate::api::AuthState;

/// OIDC 配置。
#[derive(Debug, Clone)]
pub struct OidcConfig {
    /// IdP issuer(如 `https://sso.corp/realms/main`)。
    pub issuer_url: String,
    /// 客户端 ID。
    pub client_id: String,
    /// 客户端密钥。
    pub client_secret: String,
    /// 回调地址(须与 IdP 注册一致,如 `http://localhost:8080/api/auth/oidc/callback`)。
    pub redirect_url: String,
    /// 前端地址(登录成功后带 token 跳回)。
    pub web_url: String,
}

impl OidcConfig {
    /// 从环境变量读取:`CODEPLAN_OIDC_ISSUER/CLIENT_ID/CLIENT_SECRET/REDIRECT_URL`
    /// + `CODEPLAN_WEB_URL`(可选)。前三者任一缺失 → `None`(未启用)。
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let issuer_url = std::env::var("CODEPLAN_OIDC_ISSUER").ok()?;
        let client_id = std::env::var("CODEPLAN_OIDC_CLIENT_ID").ok()?;
        let client_secret = std::env::var("CODEPLAN_OIDC_CLIENT_SECRET").ok()?;
        let redirect_url = std::env::var("CODEPLAN_OIDC_REDIRECT_URL")
            .unwrap_or_else(|_| "http://localhost:8080/api/auth/oidc/callback".to_owned());
        let web_url = std::env::var("CODEPLAN_WEB_URL")
            .unwrap_or_else(|_| "http://localhost:5173".to_owned());
        Some(Self {
            issuer_url,
            client_id,
            client_secret,
            redirect_url,
            web_url,
        })
    }
}

/// 发现并配置完成后的客户端(oauth2 5 的端点状态机:set_redirect_uri 后 AuthUrl 已设置,
/// token/userinfo 端点取决于 IdP 元数据)。
type DiscoveredClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

/// OIDC 运行时上下文:配置 + 惰性发现的客户端。
pub struct OidcCtx {
    /// 已解析配置。
    pub config: OidcConfig,
    http: reqwest::Client,
    client: OnceCell<DiscoveredClient>,
}

impl OidcCtx {
    /// 构造上下文(禁止跟随重定向,防 SSRF)。
    pub fn new(config: OidcConfig) -> Self {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        Self {
            config,
            http,
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<DiscoveredClient, AppError> {
        if let Some(c) = self.client.get() {
            return Ok(c.clone());
        }
        let client = self.discover().await?;
        let _ = self.client.set(client.clone());
        Ok(client)
    }

    async fn discover(&self) -> Result<DiscoveredClient, AppError> {
        let issuer = IssuerUrl::new(self.config.issuer_url.clone()).map_err(|e| {
            AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
        })?;
        let metadata = CoreProviderMetadata::discover_async(issuer, &self.http)
            .await
            .map_err(|e| {
                AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string()))
            })?;
        Ok(CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(self.config.client_id.clone()),
            Some(ClientSecret::new(self.config.client_secret.clone())),
        )
        .set_redirect_uri(RedirectUrl::new(self.config.redirect_url.clone()).map_err(
            |e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e.to_string())),
        )?))
    }
}

/// OIDC 路由(仅在配置启用时由 server 挂载)。
pub fn routes() -> Router<AuthState> {
    Router::new()
        .route("/api/auth/oidc/login", get(oidc_login))
        .route("/api/auth/oidc/callback", get(oidc_callback))
}

/// 发起 SSO:302 到 IdP 授权页(state = 签名 JWT,内含 nonce)。
async fn oidc_login(State(state): State<AuthState>) -> Result<Response, AppError> {
    let Some(ctx) = state.oidc.as_ref() else {
        return Err(AppError::not_found(DOMAIN_CODE, "OIDC 未启用"));
    };
    let client = ctx.client().await?;
    let nonce = uuid::Uuid::new_v4().to_string();
    let signed_state = state.auth.sign_oidc_state(&nonce)?;
    let (url, _csrf, _nonce) = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            || CsrfToken::new(signed_state),
            || Nonce::new(nonce),
        )
        .add_scope(Scope::new("email".to_owned()))
        .add_scope(Scope::new("profile".to_owned()))
        .url();
    Ok(Redirect::to(url.as_str()).into_response())
}

/// 回调查询参数。
#[derive(Debug, Deserialize)]
struct CallbackQuery {
    /// IdP 回传的授权码。
    code: String,
    /// 我们签发的防伪 state。
    state: String,
}

/// IdP 回调:换令牌 → 校验 nonce → JIT 开通 → 带会话跳回前端。
async fn oidc_callback(
    State(state): State<AuthState>,
    Query(q): Query<CallbackQuery>,
) -> Result<Response, AppError> {
    let Some(ctx) = state.oidc.as_ref() else {
        return Err(AppError::not_found(DOMAIN_CODE, "OIDC 未启用"));
    };
    let expected_nonce = Nonce::new(state.auth.verify_oidc_state(&q.state)?);
    let client = ctx.client().await?;

    let token = client
        .exchange_code(AuthorizationCode::new(q.code))
        .map_err(|_| AppError::internal(DOMAIN_CODE))?
        .request_async(&ctx.http)
        .await
        .map_err(|e| {
            AppError::new(format!("{DOMAIN_CODE}-401"), 401, "OIDC 令牌交换失败")
                .with_details(serde_json::json!(e.to_string()))
        })?;
    let id_token = token
        .id_token()
        .ok_or_else(|| AppError::new(format!("{DOMAIN_CODE}-401"), 401, "缺少 ID Token"))?;
    let claims = id_token
        .claims(&client.id_token_verifier(), &expected_nonce)
        .map_err(|_| AppError::new(format!("{DOMAIN_CODE}-401"), 401, "ID Token 校验失败"))?;

    let email = claims
        .email()
        .map(|e| e.as_str().to_owned())
        .ok_or_else(|| {
            AppError::new(
                format!("{DOMAIN_CODE}-401"),
                401,
                "ID Token 缺少 email 声明",
            )
        })?;
    let name = claims
        .preferred_username()
        .map(|u| u.as_str().to_owned())
        .unwrap_or_else(|| email.clone());

    let session = state.auth.oidc_session(&email, &name).await?;
    let target = format!(
        "{}/#/auth/callback?token={}",
        ctx.config.web_url, session.token
    );
    Ok(Redirect::to(&target).into_response())
}
