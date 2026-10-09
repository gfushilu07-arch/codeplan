//! 身份域用例:登录认证、JWT 会话签发与校验、引导管理员。

use argon2::password_hash::phc::PasswordHash;
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use chrono::{Duration, Utc};
use codeplan_common::error::AppError;
use codeplan_kernel::identity::CurrentUser;
use codeplan_kernel::ids::UserId;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::DOMAIN_CODE;
use crate::domain::{NewUser, User, UserRepository};

/// 会话有效期(小时)。
const SESSION_HOURS: i64 = 12;

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

    /// 用户名 + 密码登录,签发 JWT 会话。
    ///
    /// # Errors
    /// 凭据错误返回 `ORG-401`(统一文案,防用户名枚举);账号禁用返回 `ORG-403`。
    pub async fn login(&self, username: &str, password: &str) -> Result<Session, AppError> {
        let user = self
            .users
            .find_by_username(username)
            .await?
            .ok_or_else(|| AppError::new(format!("{DOMAIN_CODE}-401"), 401, "用户名或密码错误"))?;

        verify_password(&user, password)?;
        self.issue_session(&user)
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

    use codeplan_common::error::AppError;

    use crate::service::AuthService;
    use crate::test_support::InMemoryUserRepository;

    async fn service_with_seeded() -> AuthService {
        let repo = Arc::new(InMemoryUserRepository::new());
        repo.seed("alice", Some("correct-horse"), false, false)
            .await;
        AuthService::new(repo, "test-secret")
    }

    #[tokio::test]
    async fn login_ok_and_verify() {
        let svc = service_with_seeded().await;
        let session = svc
            .login("alice", "correct-horse")
            .await
            .expect("应登录成功");
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
