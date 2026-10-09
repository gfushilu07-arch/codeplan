//! 测试支撑:内存用户仓储(cfg(test) 专用,不得进入生产路径)。

use std::sync::Mutex;

use codeplan_common::error::AppError;
use codeplan_kernel::ids::UserId;

use crate::domain::{NewUser, User, UserRepository, UserStatus};

/// 内存用户仓储:单测用,行为与 PgUserRepository 语义对齐(唯一性/哈希)。
#[derive(Default)]
pub struct InMemoryUserRepository {
    users: Mutex<Vec<User>>,
    passwords: Mutex<Vec<String>>,
}

impl InMemoryUserRepository {
    /// 创建空仓储。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 预置一个用户并返回其 ID。
    pub async fn seed(
        &self,
        username: &str,
        password: Option<&str>,
        admin: bool,
        disabled: bool,
    ) -> User {
        let user = User {
            id: UserId::generate(),
            username: username.to_owned(),
            email: format!("{username}@codeplan.internal"),
            display_name: username.to_owned(),
            status: if disabled {
                UserStatus::Disabled
            } else {
                UserStatus::Active
            },
            password_hash: password.map(|p| crate::service::hash_password(p).expect("哈希失败")),
            is_platform_admin: admin,
            created_at: codeplan_common::time::now(),
            updated_at: codeplan_common::time::now(),
        };
        self.passwords
            .lock()
            .expect("锁")
            .push(password.unwrap_or_default().to_owned());
        self.users.lock().expect("锁").push(user.clone());
        user
    }
}

#[async_trait::async_trait]
impl UserRepository for InMemoryUserRepository {
    async fn create(&self, user: &NewUser) -> Result<User, AppError> {
        let mut users = self.users.lock().expect("锁");
        if users
            .iter()
            .any(|u| u.username == user.username || u.email == user.email)
        {
            return Err(AppError::conflict(crate::DOMAIN_CODE, "用户已存在"));
        }
        let created = User {
            id: UserId::generate(),
            username: user.username.clone(),
            email: user.email.clone(),
            display_name: user.display_name.clone(),
            status: UserStatus::Active,
            password_hash: user
                .password
                .as_deref()
                .map(crate::service::hash_password)
                .transpose()?,
            is_platform_admin: user.is_platform_admin,
            created_at: codeplan_common::time::now(),
            updated_at: codeplan_common::time::now(),
        };
        users.push(created.clone());
        Ok(created)
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<User>, AppError> {
        Ok(self
            .users
            .lock()
            .expect("锁")
            .iter()
            .find(|u| u.username == username)
            .cloned())
    }

    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, AppError> {
        Ok(self
            .users
            .lock()
            .expect("锁")
            .iter()
            .find(|u| u.id == id)
            .cloned())
    }

    async fn count(&self) -> Result<i64, AppError> {
        Ok(i64::try_from(self.users.lock().expect("锁").len()).unwrap_or(0))
    }
}
