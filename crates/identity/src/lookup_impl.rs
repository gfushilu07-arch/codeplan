//! kernel IdentityLookup 契约的 PG 实现(CODEOWNERS 自动指派等跨域查询)。

use std::sync::Arc;

use async_trait::async_trait;
use codeplan_kernel::ids::UserId;
use codeplan_kernel::lookup::{IdentityLookup, UserBrief};

use crate::domain::UserRepository;

/// PG 实现。
pub struct PgIdentityLookup {
    users: Arc<dyn UserRepository>,
}

impl PgIdentityLookup {
    /// 构造。
    #[must_use]
    pub fn new(users: Arc<dyn UserRepository>) -> Self {
        Self { users }
    }
}

#[async_trait]
impl IdentityLookup for PgIdentityLookup {
    async fn user_brief(&self, id: UserId) -> Result<Option<UserBrief>, String> {
        let u = self.users.find_by_id(id).await.map_err(|e| e.to_string())?;
        Ok(u.map(|u| UserBrief {
            id: u.id,
            display_name: u.display_name,
            email: u.email,
        }))
    }

    async fn by_username(&self, username: &str) -> Result<Option<UserBrief>, String> {
        let u = self
            .users
            .find_by_username(username)
            .await
            .map_err(|e| e.to_string())?;
        Ok(u.map(|u| UserBrief {
            id: u.id,
            display_name: u.display_name,
            email: u.email,
        }))
    }
}
