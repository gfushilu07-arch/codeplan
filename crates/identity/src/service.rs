//! 身份域用例:登录认证、JWT 会话签发与校验、引导管理员。

use argon2::password_hash::phc::PasswordHash;
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use base32::Alphabet;
use chrono::{Duration, Utc};
use codeplan_common::error::AppError;
use codeplan_kernel::identity::CurrentUser;
use codeplan_kernel::ids::UserId;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use totp_rs::{Algorithm, TOTP};

use crate::DOMAIN_CODE;
use crate::domain::{NewUser, User, UserRepository};

/// 会话有效期(小时)。
const SESSION_HOURS: i64 = 12;

/// 登录结果:直接发会话,或要求 TOTP 二次验证。
#[derive(Debug, Clone)]
pub enum LoginResult {
    /// 无 TOTP(或已完成二步)——直接发会话。
    Done(Session),
    /// 已启用 TOTP——先发短时效票据,凭验证码换会话。
    TotpRequired {
        /// 二次验证票据(JWT,mfa 待验证,5 分钟)。
        ticket: String,
    },
}

/// 登录成功后的会话。
#[derive(Debug, Clone)]
pub struct Session {
    /// JWT 字符串。
    pub token: String,
    /// 过期时间(UTC)。
    pub expires_at: chrono::DateTime<Utc>,
    /// 用户 ID。
    pub user_id: UserId,
    /// 显示名。
    pub display_name: String,
    /// 是否平台管理员。
    pub is_platform_admin: bool,
}

/// MFA 待验证票据 Claims(短时效)。
#[derive(Debug, Serialize, Deserialize)]
struct MfaClaims {
    /// 主体(用户 ID 字符串)。
    sub: String,
    /// 恒为 true。
    mfa: bool,
    /// 过期时间(Unix 秒)。
    exp: i64,
    /// 签发时间(Unix 秒)。
    iat: i64,
}

/// JWT Claims。
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    /// 主体(用户 ID 字符串)。
    sub: String,
    /// 显示名。
    name: String,
    /// 是否平台管理员。
    admin: bool,
    /// 过期时间(Unix 秒)。
    exp: i64,
    /// 签发时间(Unix 秒)。
    iat: i64,
}

/// 认证服务:依赖用户仓储与 JWT 密钥。
pub struct AuthService {
    users: std::sync::Arc<dyn UserRepository>,
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
}

impl AuthService {
    /// 构造认证服务。
    #[must_use]
    pub fn new(users: std::sync::Arc<dyn UserRepository>, jwt_secret: &str) -> Self {
        Self {
            users,
            encoding_key: EncodingKey::from_secret(jwt_secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(jwt_secret.as_bytes()),
        }
    }

    /// 用户名 + 密码登录:已启用 TOTP 的用户返回二次验证票据,否则直接发会话。
    ///
    /// # Errors
    /// 凭据错误返回 `ORG-401`(统一文案,防用户名枚举);账号禁用返回 `ORG-403`。
    pub async fn login(&self, username: &str, password: &str) -> Result<LoginResult, AppError> {
        let user = self
            .users
            .find_by_username(username)
            .await?
            .ok_or_else(|| AppError::new(format!("{DOMAIN_CODE}-401"), 401, "用户名或密码错误"))?;

        verify_password(&user, password)?;
        if user.totp_enabled {
            let ticket = self.sign_mfa_ticket(&user.id)?;
            return Ok(LoginResult::TotpRequired { ticket });
        }
        Ok(LoginResult::Done(self.issue_session(&user)?))
    }

    /// 凭 MFA 票据 + TOTP 验证码换取正式会话。
    ///
    /// # Errors
    /// 票据无效返回 `ORG-401`;验证码错误返回 `ORG-401`。
    pub async fn verify_totp_login(&self, ticket: &str, code: &str) -> Result<Session, AppError> {
        let claims: MfaClaims = self
            .decoding_key_decode(ticket)
            .ok_or_else(|| AppError::new(format!("{DOMAIN_CODE}-401"), 401, "票据无效或已过期"))?;
        if !claims.mfa {
            return Err(AppError::new(
                format!("{DOMAIN_CODE}-401"),
                401,
                "票据无效或已过期",
            ));
        }
        let user_id = UserId::try_from(claims.sub.as_str())
            .map_err(|_| AppError::new(format!("{DOMAIN_CODE}-401"), 401, "票据无效或已过期"))?;
        let user =
            self.users.find_by_id(user_id).await?.ok_or_else(|| {
                AppError::new(format!("{DOMAIN_CODE}-401"), 401, "票据无效或已过期")
            })?;
        if !verify_totp_code(user.totp_secret.as_deref(), code) {
            return Err(AppError::new(
                format!("{DOMAIN_CODE}-401"),
                401,
                "验证码错误",
            ));
        }
        self.issue_session(&user)
    }

    /// 生成 TOTP 密钥(待确认),返回 base32 密钥与 otpauth URL。
    ///
    /// # Errors
    /// 存储故障或密钥编码失败。
    pub async fn totp_setup(
        &self,
        user: UserId,
        account_label: &str,
    ) -> Result<(String, String), AppError> {
        let mut raw = [0u8; 20];
        raw[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        raw[16..].copy_from_slice(&uuid::Uuid::new_v4().as_bytes()[..4]);
        let secret = base32::encode(Alphabet::Rfc4648 { padding: false }, &raw);
        let url = format!(
            "otpauth://totp/CodePlan:{account_label}?secret={secret}&issuer=CodePlan\
             &algorithm=SHA1&digits=6&period=30"
        );
        self.users
            .save_totp(user, Some(secret.clone()), false)
            .await?;
        Ok((secret, url))
    }

    /// 确认启用 TOTP(验证码通过才生效)。
    ///
    /// # Errors
    /// 未 setup 返回 `ORG-400`;验证码错误返回 `ORG-401`。
    pub async fn totp_enable(&self, user: UserId, code: &str) -> Result<(), AppError> {
        let u = self
            .users
            .find_by_id(user)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "用户不存在"))?;
        let Some(secret) = u.totp_secret.as_deref() else {
            return Err(AppError::invalid(DOMAIN_CODE, "请先调用 setup 获取密钥"));
        };
        if !verify_totp_code(Some(secret), code) {
            return Err(AppError::new(
                format!("{DOMAIN_CODE}-401"),
                401,
                "验证码错误",
            ));
        }
        self.users.save_totp(user, u.totp_secret, true).await
    }

    /// 关闭 TOTP(需验证码确认)。
    ///
    /// # Errors
    /// 未启用返回 `ORG-400`;验证码错误返回 `ORG-401`。
    pub async fn totp_disable(&self, user: UserId, code: &str) -> Result<(), AppError> {
        let u = self
            .users
            .find_by_id(user)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "用户不存在"))?;
        if !u.totp_enabled {
            return Err(AppError::invalid(DOMAIN_CODE, "TOTP 未启用"));
        }
        if !verify_totp_code(u.totp_secret.as_deref(), code) {
            return Err(AppError::new(
                format!("{DOMAIN_CODE}-401"),
                401,
                "验证码错误",
            ));
        }
        self.users.save_totp(user, None, false).await
    }

    /// OIDC JIT:按邮箱找用户,不存在则开通(无本地密码),签发会话。
    ///
    /// # Errors
    /// 账号禁用返回 `ORG-403`;创建冲突/存储故障返回对应错误。
    pub async fn oidc_session(&self, email: &str, display_name: &str) -> Result<Session, AppError> {
        let existing = self.users.find_by_email(email).await?;
        let user = match existing {
            Some(u) => u,
            None => {
                self.users
                    .create(&NewUser {
                        username: email.to_owned(),
                        email: email.to_owned(),
                        display_name: display_name.to_owned(),
                        password: None,
                        is_platform_admin: false,
                    })
                    .await?
            }
        };
        if user.status == crate::domain::UserStatus::Disabled {
            return Err(AppError::forbidden(DOMAIN_CODE, "账号已禁用"));
        }
        self.issue_session(&user)
    }

    /// 签发 OIDC 回调防伪 state(内含 nonce,5 分钟)。
    ///
    /// # Errors
    /// 签名失败。
    pub fn sign_oidc_state(&self, nonce: &str) -> Result<String, AppError> {
        let claims = MfaClaims {
            sub: nonce.to_owned(),
            mfa: true,
            exp: (Utc::now() + Duration::minutes(5)).timestamp(),
            iat: Utc::now().timestamp(),
        };
        jsonwebtoken::encode(&Header::default(), &claims, &self.encoding_key)
            .map_err(|_| AppError::internal(DOMAIN_CODE))
    }

    /// 校验 OIDC 回调 state,返回 nonce。
    ///
    /// # Errors
    /// 签名/过期返回 `ORG-400`。
    pub fn verify_oidc_state(&self, token: &str) -> Result<String, AppError> {
        let claims: MfaClaims = self
            .decoding_key_decode(token)
            .ok_or_else(|| AppError::invalid(DOMAIN_CODE, "state 无效或已过期"))?;
        if !claims.mfa {
            return Err(AppError::invalid(DOMAIN_CODE, "state 无效或已过期"));
        }
        Ok(claims.sub)
    }

    fn sign_mfa_ticket(&self, user_id: &UserId) -> Result<String, AppError> {
        let claims = MfaClaims {
            sub: user_id.as_uuid().to_string(),
            mfa: true,
            exp: (Utc::now() + Duration::minutes(5)).timestamp(),
            iat: Utc::now().timestamp(),
        };
        jsonwebtoken::encode(&Header::default(), &claims, &self.encoding_key)
            .map_err(|_| AppError::internal(DOMAIN_CODE))
    }

    fn decoding_key_decode<T: serde::de::DeserializeOwned>(&self, token: &str) -> Option<T> {
        jsonwebtoken::decode::<T>(token, &self.decoding_key, &Validation::default())
            .ok()
            .map(|d| d.claims)
    }

    /// 校验 JWT 并还原当前用户上下文。
    ///
    /// # Errors
    /// 令牌无效/过期返回 `ORG-401`;用户已被删除或禁用返回 `ORG-401`。
    pub async fn verify_token(&self, token: &str) -> Result<CurrentUser, AppError> {
        let data =
            jsonwebtoken::decode::<Claims>(token, &self.decoding_key, &Validation::default())
                .map_err(|_| {
                    AppError::new(format!("{DOMAIN_CODE}-401"), 401, "会话无效或已过期")
                })?;

        let user_id = UserId::try_from(data.claims.sub.as_str())
            .map_err(|_| AppError::new(format!("{DOMAIN_CODE}-401"), 401, "会话无效或已过期"))?;
        let user =
            self.users.find_by_id(user_id).await?.ok_or_else(|| {
                AppError::new(format!("{DOMAIN_CODE}-401"), 401, "会话无效或已过期")
            })?;
        if user.status == crate::domain::UserStatus::Disabled {
            return Err(AppError::forbidden(DOMAIN_CODE, "账号已禁用"));
        }

        let current = CurrentUser::new(user.id, user.display_name.clone(), user.email);
        Ok(if user.is_platform_admin {
            current.as_platform_admin()
        } else {
            current
        })
    }

    /// 用户数为 0 时创建引导管理员(用户名 `admin`)。
    ///
    /// # Errors
    /// 存储故障。
    pub async fn bootstrap_admin_if_empty(&self, password: &str) -> Result<(), AppError> {
        if self.users.count().await? > 0 {
            return Ok(());
        }
        self.users
            .create(&NewUser {
                username: "admin".to_owned(),
                email: "admin@codeplan.internal".to_owned(),
                display_name: "Administrator".to_owned(),
                password: Some(password.to_owned()),
                is_platform_admin: true,
            })
            .await?;
        Ok(())
    }

    fn issue_session(&self, user: &User) -> Result<Session, AppError> {
        let now = Utc::now();
        let expires_at = now + Duration::hours(SESSION_HOURS);
        let claims = Claims {
            sub: user.id.as_uuid().to_string(),
            name: user.display_name.clone(),
            admin: user.is_platform_admin,
            exp: expires_at.timestamp(),
            iat: now.timestamp(),
        };
        let token = jsonwebtoken::encode(&Header::default(), &claims, &self.encoding_key)
            .map_err(|_| AppError::internal(DOMAIN_CODE))?;
        Ok(Session {
            token,
            expires_at,
            user_id: user.id,
            display_name: user.display_name.clone(),
            is_platform_admin: user.is_platform_admin,
        })
    }
}

fn verify_password(user: &User, password: &str) -> Result<(), AppError> {
    if user.status == crate::domain::UserStatus::Disabled {
        return Err(AppError::forbidden(DOMAIN_CODE, "账号已禁用"));
    }
    let Some(hash) = user.password_hash.as_deref() else {
        // 无本地密码(如 LDAP 同步账号):按凭据错误处理,防探测
        return Err(AppError::new(
            format!("{DOMAIN_CODE}-401"),
            401,
            "用户名或密码错误",
        ));
    };
    let parsed = PasswordHash::new(hash).map_err(|_| AppError::internal(DOMAIN_CODE))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| AppError::new(format!("{DOMAIN_CODE}-401"), 401, "用户名或密码错误"))
}

/// 哈希明文密码(argon2 默认参数,16 字节随机盐)。
///
/// # Errors
/// 盐生成或哈希失败。
pub(crate) fn hash_password(password: &str) -> Result<String, AppError> {
    // 122 位随机数 ×2 作为盐(避免额外引入 rand_core 的 os 特性链)
    let mut salt = [0u8; 16];
    salt[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    let hash = Argon2::default()
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map_err(|_| AppError::internal(DOMAIN_CODE))?;
    Ok(hash.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use base32::Alphabet;
    use totp_rs::{Algorithm, TOTP};

    use crate::service::AuthService;
    use crate::test_support::InMemoryUserRepository;

    fn service_with_1user() -> AuthService {
        let repo = Arc::new(InMemoryUserRepository::new());
        AuthService::new(repo, "test-secret")
    }

    async fn service_with_seeded() -> AuthService {
        let repo = Arc::new(InMemoryUserRepository::new());
        repo.seed("alice", Some("correct-horse"), false, false)
            .await;
        AuthService::new(repo, "test-secret")
    }

    #[tokio::test]
    async fn login_ok_and_verify() {
        let svc = service_with_seeded().await;
        let result = svc
            .login("alice", "correct-horse")
            .await
            .expect("应登录成功");
        let crate::service::LoginResult::Done(session) = result else {
            panic!("未启用 TOTP 应直接发会话");
        };
        let me = svc.verify_token(&session.token).await.expect("令牌应有效");
        assert_eq!(me.display_name, "alice");
        assert!(!me.is_platform_admin);
    }

    #[tokio::test]
    async fn login_wrong_password_is_401() {
        let svc = service_with_seeded().await;
        let err = svc.login("alice", "nope").await.expect_err("应失败");
        assert_eq!(err.code(), "ORG-401");
    }

    #[tokio::test]
    async fn totp_full_flow() {
        let repo = Arc::new(InMemoryUserRepository::new());
        repo.seed("carol", Some("pw"), false, false).await;
        let svc = AuthService::new(repo, "test-secret");
        let user_id = {
            let crate::service::LoginResult::Done(s) = svc.login("carol", "pw").await.expect("ok")
            else {
                panic!("未启用 TOTP");
            };
            s.user_id
        };

        // setup → 用密钥算当前验证码 → enable
        let (secret, url) = svc
            .totp_setup(user_id, "carol@codeplan.internal")
            .await
            .expect("setup");
        assert!(url.starts_with("otpauth://totp/CodePlan:"));
        let code = current_totp_code(&secret);
        svc.totp_enable(user_id, &code).await.expect("enable");

        // 登录 → 应要求二次验证
        let result = svc.login("carol", "pw").await.expect("ok");
        let crate::service::LoginResult::TotpRequired { ticket } = result else {
            panic!("已启用 TOTP 应要求二次验证");
        };

        // 错误验证码 → 401;正确验证码 → 会话
        let err = svc
            .verify_totp_login(&ticket, "000000")
            .await
            .expect_err("错误验证码应失败");
        assert_eq!(err.code(), "ORG-401");

        let code2 = current_totp_code(&secret);
        let session = svc
            .verify_totp_login(&ticket, &code2)
            .await
            .expect("正确验证码应换到会话");
        assert_eq!(session.display_name, "carol");

        // disable 后登录恢复直连
        let code3 = current_totp_code(&secret);
        svc.totp_disable(user_id, &code3).await.expect("disable");
        let crate::service::LoginResult::Done(_) = svc.login("carol", "pw").await.expect("ok")
        else {
            panic!("关闭 TOTP 后应直接发会话");
        };
    }

    #[tokio::test]
    async fn oidc_state_roundtrip() {
        let svc = service_with_1user();
        let signed = svc.sign_oidc_state("nonce-123").expect("签名");
        assert_eq!(
            svc.verify_oidc_state(&signed).expect("应验签通过"),
            "nonce-123"
        );
        assert!(svc.verify_oidc_state("tampered").is_err());
    }

    #[tokio::test]
    async fn oidc_jit_provisioning() {
        let svc = service_with_1user();
        let s = svc
            .oidc_session("newcomer@corp.example", "Newcomer")
            .await
            .expect("JIT 开通应成功");
        assert_eq!(s.display_name, "Newcomer");
        // 二次登录应复用同一账号
        let s2 = svc
            .oidc_session("newcomer@corp.example", "Newcomer")
            .await
            .expect("ok");
        assert_eq!(s.user_id, s2.user_id);
    }

    // 与 totp-rs 相同参数计算当前验证码(测试夹具)
    fn current_totp_code(secret_b32: &str) -> String {
        let raw = base32::decode(Alphabet::Rfc4648 { padding: false }, secret_b32)
            .expect("密钥应为合法 base32");
        let totp = TOTP::new(Algorithm::SHA1, 6, 1, 30, raw).expect("TOTP 参数合法");
        totp.generate_current().expect("生成验证码失败")
    }

    #[tokio::test]
    async fn login_unknown_user_same_message() {
        // 防用户名枚举:未知用户与密码错误的文案一致
        let svc = service_with_seeded().await;
        let e1 = svc.login("ghost", "x").await.expect_err("应失败");
        let e2 = svc.login("alice", "wrong").await.expect_err("应失败");
        assert_eq!(e1.message(), e2.message());
    }

    #[tokio::test]
    async fn disabled_user_cannot_login() {
        let repo = Arc::new(InMemoryUserRepository::new());
        repo.seed("bob", Some("pw"), false, true).await;
        let svc = AuthService::new(repo, "test-secret");
        let err = svc.login("bob", "pw").await.expect_err("禁用账号应失败");
        assert_eq!(err.code(), "ORG-403");
    }

    #[tokio::test]
    async fn bootstrap_admin_only_when_empty() {
        let repo = Arc::new(InMemoryUserRepository::new());
        let svc = AuthService::new(repo, "test-secret");
        svc.bootstrap_admin_if_empty("first-pw")
            .await
            .expect("首次应创建");
        svc.bootstrap_admin_if_empty("second-pw")
            .await
            .expect("非空应跳过");
        let admin = svc.login("admin", "first-pw").await;
        assert!(admin.is_ok(), "引导密码不应被第二次调用覆盖");
    }

    #[tokio::test]
    async fn garbage_token_rejected() {
        let repo = Arc::new(InMemoryUserRepository::new());
        let svc = AuthService::new(repo, "test-secret");
        let err = svc.verify_token("garbage").await.expect_err("应拒绝");
        assert_eq!(err.code(), "ORG-401");
    }
}

/// 校验 TOTP 验证码(±1 周期容差;无密钥一律失败)。
fn verify_totp_code(secret: Option<&str>, code: &str) -> bool {
    let Some(secret) = secret else {
        return false;
    };
    let Some(raw) = base32::decode(Alphabet::Rfc4648 { padding: false }, secret) else {
        return false;
    };
    let Ok(totp) = TOTP::new(Algorithm::SHA1, 6, 1, 30, raw) else {
        return false;
    };
    totp.check_current(code).unwrap_or(false)
}
