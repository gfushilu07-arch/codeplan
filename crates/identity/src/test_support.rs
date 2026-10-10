//! 测试支撑:内存用户仓储(cfg(test) 专用,不得进入生产路径)。

use std::sync::{Arc, Mutex};

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
            source: crate::domain::UserSource::Local,
            totp_secret: None,
            totp_enabled: false,
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
            source: user.source,
            totp_secret: None,
            totp_enabled: false,
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

    async fn list(&self) -> Result<Vec<User>, AppError> {
        Ok(self.users.lock().expect("锁").clone())
    }

    async fn set_status(&self, id: UserId, status: UserStatus) -> Result<(), AppError> {
        if let Some(u) = self
            .users
            .lock()
            .expect("锁")
            .iter_mut()
            .find(|u| u.id == id)
        {
            u.status = status;
            u.updated_at = codeplan_common::time::now();
        }
        Ok(())
    }

    async fn find_by_email(&self, email: &str) -> Result<Option<User>, AppError> {
        Ok(self
            .users
            .lock()
            .expect("锁")
            .iter()
            .find(|u| u.email == email)
            .cloned())
    }

    async fn upsert_ldap_user(
        &self,
        username: &str,
        email: &str,
        display_name: &str,
    ) -> Result<(), AppError> {
        let mut users = self.users.lock().expect("锁");
        if let Some(u) = users.iter_mut().find(|u| u.username == username) {
            u.email = email.to_owned();
            u.display_name = display_name.to_owned();
            u.source = crate::domain::UserSource::Ldap;
            u.updated_at = codeplan_common::time::now();
        } else {
            users.push(User {
                id: UserId::generate(),
                username: username.to_owned(),
                email: email.to_owned(),
                display_name: display_name.to_owned(),
                status: UserStatus::Active,
                password_hash: None,
                source: crate::domain::UserSource::Ldap,
                totp_secret: None,
                totp_enabled: false,
                is_platform_admin: false,
                created_at: codeplan_common::time::now(),
                updated_at: codeplan_common::time::now(),
            });
        }
        Ok(())
    }

    async fn save_totp(
        &self,
        id: UserId,
        secret: Option<String>,
        enabled: bool,
    ) -> Result<(), AppError> {
        let mut users = self.users.lock().expect("锁");
        if let Some(u) = users.iter_mut().find(|u| u.id == id) {
            u.totp_secret = secret;
            u.totp_enabled = enabled;
            u.updated_at = codeplan_common::time::now();
        }
        Ok(())
    }
}

/// 内存资源角色存储(与用户表共享,便于 members 透出展示信息)。
pub struct InMemoryRbacStore {
    users: Arc<InMemoryUserRepository>,
    entries: Mutex<Vec<(UserId, String, uuid::Uuid, crate::domain::ResourceRole)>>,
}

impl InMemoryRbacStore {
    /// 绑定用户表创建空存储。
    #[must_use]
    pub fn new(users: Arc<InMemoryUserRepository>) -> Self {
        Self {
            users,
            entries: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl crate::domain::RbacStore for InMemoryRbacStore {
    async fn grant(
        &self,
        user_id: UserId,
        kind: &str,
        resource_id: uuid::Uuid,
        role: crate::domain::ResourceRole,
    ) -> Result<(), AppError> {
        let mut e = self.entries.lock().expect("锁");
        if let Some(slot) = e
            .iter_mut()
            .find(|(u, k, r, _)| *u == user_id && k == kind && *r == resource_id)
        {
            slot.3 = role;
        } else {
            e.push((user_id, kind.to_owned(), resource_id, role));
        }
        Ok(())
    }

    async fn revoke(
        &self,
        user_id: UserId,
        kind: &str,
        resource_id: uuid::Uuid,
    ) -> Result<(), AppError> {
        self.entries
            .lock()
            .expect("锁")
            .retain(|(u, k, r, _)| !(*u == user_id && k == kind && *r == resource_id));
        Ok(())
    }

    async fn role_of(
        &self,
        user_id: UserId,
        kind: &str,
        resource_id: uuid::Uuid,
    ) -> Result<Option<crate::domain::ResourceRole>, AppError> {
        Ok(self
            .entries
            .lock()
            .expect("锁")
            .iter()
            .find(|(u, k, r, _)| *u == user_id && k == kind && *r == resource_id)
            .map(|(_, _, _, role)| *role))
    }

    async fn members(
        &self,
        kind: &str,
        resource_id: uuid::Uuid,
    ) -> Result<Vec<crate::domain::MemberEntry>, AppError> {
        let users = self.users.users.lock().expect("锁");
        Ok(self
            .entries
            .lock()
            .expect("锁")
            .iter()
            .filter(|(_, k, r, _)| k == kind && *r == resource_id)
            .filter_map(|(u, _, _, role)| {
                users
                    .iter()
                    .find(|user| user.id == *u)
                    .map(|user| crate::domain::MemberEntry {
                        user_id: user.id,
                        username: user.username.clone(),
                        display_name: user.display_name.clone(),
                        role: *role,
                    })
            })
            .collect())
    }
}
